mod args;
mod deps;
mod payload;
mod pipeline;
mod sign;
mod sisr;
mod stub;

pub(crate) use args::{load_config, BuildArgs, BuildPlan, GpuArg};
use payload::{count_files, print_tree};
use pipeline::{build_single_target, build_universal, warn_sandbox_noops};

use anyhow::{Context, Result};
use daedalus_core::detect;
use std::path::Path;

/// run - run.
/// @args: command arguments
/// @verbose: verbose
///
/// Description:
///
/// Return: Result containing Result<()>
pub fn run(args: BuildArgs, verbose: bool) -> Result<()> {
    // Quiet mode overrides verbose
    let verbose = verbose && !args.quiet;

    let app_dir = args.app.canonicalize().with_context(|| {
        format!(
            "app directory not found: {}\n\
             Check that the path exists and try again.",
            args.app.display()
        )
    })?;
    if !app_dir.is_dir() {
        anyhow::bail!(
            "{} is not a directory\n\
             daedalus expects a directory containing an app (e.g. requirements.txt, package.json).",
            app_dir.display()
        );
    }

    // Load .daedalus.toml config if present
    let config = load_config(&app_dir);

    // Resolve runtime + model id before any partial field moves of `config`
    // below, so the helper can borrow-build the id without tripping the
    // borrow checker on the whole-struct reference.
    let model_id_from_config = config.build.model_id.clone();
    let (runtime, model_id) =
        resolve_build_runtime(&app_dir, &args, model_id_from_config, verbose)?;
    let runtime_name = runtime.name().to_string();

    // Apply config defaults (CLI flags override). Clone the args fields so
    // `&args` stays borrowable for the per-target loop below.
    let isolation = if args.isolation != "sandbox" {
        args.isolation.clone()
    } else {
        config.build.isolation.unwrap_or_else(|| "sandbox".into())
    };
    let isolation_num = args::parse_isolation(&isolation)
        .with_context(|| format!("invalid --isolation value: '{isolation}'"))?;

    let targets = args::resolve_targets(&args, config.build.target.as_deref());
    let outputs = args::output_paths(&args, &targets);
    let (entrypoint_args, mut services) = args::parse_entrypoints(&args.entrypoint);
    args::apply_service_overrides(&mut services, &args.service_port, &args.service_timeout)?;
    let gpu_backend = resolve_gpu_backend(args.gpu, config.build.gpu.as_deref())?;

    let plan = BuildPlan {
        verbose,
        app_dir,
        runtime,
        runtime_name,
        isolation,
        isolation_num,
        no_install: args.no_install || config.build.no_install.unwrap_or(false),
        seccomp: args.seccomp || config.build.seccomp.unwrap_or(false),
        landlock: args.landlock || config.build.landlock.unwrap_or(false),
        gui: args.gui || config.build.gui.unwrap_or(false),
        gpu_backend,
        cpu_limit: args.cpu_limit,
        memory_limit_mb: args.memory_limit_mb,
        pid_limit: args.pid_limit,
        pre_hooks: args.pre_hooks.clone(),
        post_hooks: args.post_hooks.clone(),
        squashfs: args.squashfs || config.build.squashfs.unwrap_or(false),
        version_info: args.version_info.clone().or(config.package.version),
        author: args.author.clone().or(config.package.author),
        description: args.description.clone().or(config.package.description),
        license: args.license.clone().or(config.package.license),
        env_file: args
            .env_file
            .clone()
            .or(config.build.env_file.map(std::path::PathBuf::from)),
        targets,
        outputs,
        services,
        entrypoint: entrypoint_args,
        model_id,
    };

    if args.dry_run {
        for (target, output) in plan.targets.iter().zip(&plan.outputs) {
            print_dry_run(&args, &plan, target.as_deref(), output);
        }
        return Ok(());
    }

    // Universal binary: build multi-arch slices and assemble into one file.
    if args.universal {
        return build_universal(&args, &plan, &args.output);
    }

    // Build one artifact per target; each gets its own output path.
    let mut json_results: Vec<serde_json::Value> = Vec::new();
    for (target, output) in plan.targets.iter().zip(&plan.outputs) {
        if let Some(result) = build_single_target(&args, &plan, target.clone(), output)? {
            json_results.push(result);
        }
    }

    // Phase 4: optionally publish layers to a content-addressable registry
    if let Some(registry_url) = &args.publish {
        if registry_url.contains("daedalus.example.com") {
            anyhow::bail!("cannot use placeholder registry URL '{registry_url}'");
        }
        for output in &plan.outputs {
            if output.exists() {
                publish_artifact_to_registry(output, registry_url, args.token.as_deref(), verbose)?;
            }
        }
    }

    if args.json {
        let doc = if json_results.len() == 1 {
            json_results.remove(0)
        } else {
            serde_json::Value::Array(json_results)
        };
        println!("{}", serde_json::to_string_pretty(&doc)?);
    }
    Ok(())
}

/// Resolve the build runtime and model id.
///
/// `--model` implies an offline Gemma bundle: embedding weights for local
/// inference pins the runtime to Gemma regardless of the source layout, so the
/// stub serves the model from the bundled `.gguf`. The model id is derived from
/// the `--model` filename (stable across rebuilds of the same weights) and falls
/// back to `[build] model_id` from `.daedalus.toml`.
fn resolve_build_runtime(
    app_dir: &Path,
    args: &BuildArgs,
    model_id_from_config: Option<String>,
    verbose: bool,
) -> Result<(detect::Runtime, Option<String>)> {
    let runtime = if args.model.is_some() {
        detect::Runtime::Gemma
    } else {
        detect::detect_runtime(app_dir).context(
            "could not detect runtime — supported: python, node, deno, java, ruby, dotnet, go, php, perl, hugo, ollama, gemma, wasm, binary",
        )?
    };
    if verbose {
        eprintln!("Detected runtime: {}", runtime.name());
    }
    let model_id = args
        .model
        .as_ref()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .or(model_id_from_config);
    Ok((runtime, model_id))
}

/// publish_artifact_to_registry - publish artifact to registry.
///
/// Description:
///
/// Return: nothing
fn publish_artifact_to_registry(
    artifact: &Path,
    registry_url: &str,
    token: Option<&str>,
    verbose: bool,
) -> Result<()> {
    let is_remote = registry_url.starts_with("http://") || registry_url.starts_with("https://");

    if is_remote {
        // Remote HTTP registry — upload the full .daedalus binary
        crate::commands::registry::push_remote_artifact(registry_url, artifact, token, verbose)?;
        return Ok(());
    }

    // Local directory registry — extract layers via LayerRegistry
    let path = expand_path(registry_url);
    std::fs::create_dir_all(&path)
        .with_context(|| format!("failed to create registry dir {}", path.display()))?;
    let mut reg = daedalus_core::registry::LayerRegistry::disk(&path)
        .with_context(|| format!("failed to open local registry {}", path.display()))?;

    let (_footer, layers) = crate::commands::registry::extract_layers_from_artifact(artifact)
        .with_context(|| format!("failed to parse layers from {}", artifact.display()))?;

    if layers.is_empty() {
        if verbose {
            eprintln!(
                "[daedalus] no layers to publish from {}",
                artifact.display()
            );
        }
        return Ok(());
    }

    for layer in &layers {
        let hash = reg.push_layer(layer)?;
        eprintln!("  pushed layer '{}' -> {hash}", layer.name());
    }

    let manifest = daedalus_core::registry::LayerManifest {
        artifact_name: artifact
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("artifact")
            .to_string(),
        layers: build_layer_refs(&mut reg, &layers)?,
    };
    let manifest_hash = reg.publish_artifact(&manifest)?;
    eprintln!("  published artifact manifest -> {manifest_hash}");
    Ok(())
}

/// build_layer_refs - build layer refs.
///
/// Description:
///
/// Return: nothing
fn build_layer_refs(
    reg: &mut daedalus_core::registry::LayerRegistry,
    layers: &[daedalus_core::layer::SerializableLayer],
) -> Result<Vec<daedalus_core::registry::LayerRef>> {
    let mut refs = vec![];
    for layer in layers {
        let hex = reg.push_layer(layer)?;
        let serialized =
            serde_json::to_vec(layer).map_err(|e| anyhow::anyhow!("serialize layer: {e}"))?;
        refs.push(daedalus_core::registry::LayerRef {
            hash: hex,
            name: layer.name().to_string(),
            kind: layer.kind(),
            size: serialized.len(),
        });
    }
    Ok(refs)
}

/// expand_path - expand path.
/// @path: file or directory path
/// @std: std
/// @path: file or directory path
///
/// Description:
///
/// Return: the std::path::PathBuf
fn expand_path(path: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(path);
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return std::path::PathBuf::from(home).join(rest);
        }
    }
    p.to_path_buf()
}

/// print_dry_run - print dry run.
/// @args: command arguments
/// @plan: plan
/// @target: target
/// @output: output destination
///
/// Description:
///
/// Return: nothing
fn print_dry_run(args: &BuildArgs, plan: &BuildPlan, target: Option<&str>, output: &Path) {
    eprintln!("Dry run — would build:");
    eprintln!("  App:       {}", plan.app_dir.display());
    eprintln!("  Output:    {}", output.display());
    eprintln!("  Runtime:   {}", plan.runtime_name);
    eprintln!("  Isolation: {}", plan.isolation);
    eprintln!("  Seccomp:   {}", plan.seccomp);
    eprintln!("  Landlock:  {}", plan.landlock);
    eprintln!("  GUI:       {}", plan.gui);
    eprintln!(
        "  GPU:       {}",
        if plan.gpu_backend.is_empty() {
            "none (CPU)"
        } else {
            &plan.gpu_backend
        }
    );
    warn_sandbox_noops(plan.isolation_num, plan.seccomp, plan.landlock);
    eprintln!("  SquashFS:  {}", plan.squashfs);
    if args.enable_sisr {
        eprintln!("  SISR:      enabled (delta-indexed, <output>.manifest)");
        match &args.update_url {
            Some(url) => eprintln!("  Update URL: {url}"),
            None => {
                eprintln!(
                    "  Update URL: (none — updates must pass a URL or set DAEDALUS_UPDATE_URL)"
                );
            }
        }
    }
    if let Some(ref v) = plan.version_info {
        eprintln!("  Version:   {v}");
    }
    if let Some(ref a) = plan.author {
        eprintln!("  Author:    {a}");
    }
    if let Some(ref d) = plan.description {
        eprintln!("  Desc:      {d}");
    }
    if let Some(ref l) = plan.license {
        eprintln!("  License:   {l}");
    }
    if let Some(t) = target {
        eprintln!("  Target:    {t}");
    }
    if let Some(ref e) = plan.env_file {
        eprintln!("  Env file:  {}", e.display());
    }
    if plan.no_install {
        eprintln!("  No install: yes");
    }
    if args.update {
        eprintln!("  Update:    yes (incremental)");
    }
    if args.tree_shake {
        eprintln!("  Tree-shake: yes");
    }
    if args.minify {
        eprintln!("  Minify:    yes");
    }
    if args.persist {
        eprintln!("  Persist:   yes");
    }
    if let Some(port) = args.health_port {
        eprintln!("  Health:    port {port}");
    }
    if !args.cron.is_empty() {
        eprintln!("  Cron:      {} task(s)", args.cron.len());
    }
    if !args.include.is_empty() {
        for inc in &args.include {
            eprintln!("  Include:   {}", inc.display());
        }
    }
    if let Some(p) = &args.publish {
        eprintln!("  Publish:   {p}");
    }
    if let Some(model) = &args.model {
        eprintln!("  AI model:  {} (mode: offline, embedded)", model.display());
    }
    if !plan.services.is_empty() {
        eprintln!("  Services:  {}", describe_services(plan));
    }

    // Detect package managers
    let all_mgrs = daedalus_core::pkgmgr::detect_all_pkgmgrs(&plan.app_dir, &plan.runtime_name);
    if all_mgrs.is_empty() {
        eprintln!("  Pkg mgr:   (none)");
    } else {
        for mgr in &all_mgrs {
            eprintln!("  Pkg mgr:   {}", mgr.name());
            if !plan.no_install {
                let cmd = mgr.install_cmd();
                eprintln!("  Install:   {}", cmd.join(" "));
            }
        }
    }

    // Estimate sizes
    let file_count = count_files(&plan.app_dir);
    eprintln!("  Files:     {file_count}");

    if plan.verbose {
        eprintln!("\nFile tree:");
        print_tree(&plan.app_dir, 2);
    }
}

/// Resolve the requested GPU backend to a metadata value: `""` (CPU),
/// `"nvidia"`, or `"rocm"`. `--gpu auto` probes the build host; explicit
/// values pass through. A missing `.daedalus.toml` value means CPU.
fn resolve_gpu_backend(flag: Option<GpuArg>, config: Option<&str>) -> Result<String> {
    let requested = match flag {
        Some(g) => g,
        None => match config {
            Some(raw) => parse_gpu_arg(raw)?,
            None => return Ok(String::new()),
        },
    };
    match requested {
        GpuArg::None => Ok(String::new()),
        GpuArg::Nvidia => Ok("nvidia".into()),
        GpuArg::Rocm => Ok("rocm".into()),
        GpuArg::Auto => match daedalus_core::gpu::detect_gpu().backend {
            Some(b) => Ok(b.as_str().into()),
            None => {
                eprintln!(
                    "[daedalus] warning: --gpu auto found no accelerator — building CPU-only"
                );
                Ok(String::new())
            }
        },
    }
}

/// Parse a backend name from `.daedalus.toml` `[build] gpu = "..."`.
fn parse_gpu_arg(raw: &str) -> Result<GpuArg> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "auto" => Ok(GpuArg::Auto),
        "nvidia" => Ok(GpuArg::Nvidia),
        "rocm" => Ok(GpuArg::Rocm),
        "none" => Ok(GpuArg::None),
        other => {
            anyhow::bail!("invalid [build] gpu value '{other}' (expected auto|nvidia|rocm|none)")
        }
    }
}

/// Render the multi-service list as a single human-readable summary line.
fn describe_services(plan: &BuildPlan) -> String {
    plan.services
        .iter()
        .map(|s| {
            let mut desc = format!("{}={}", s.name, s.cmd.join(","));
            if s.ready_port != 0 {
                desc.push_str(&format!(" (ready :{})", s.ready_port));
            }
            desc
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// gpu_backend_explicit_values_pass_through - explicit backends.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn gpu_backend_explicit_values_pass_through() {
        assert_eq!(
            resolve_gpu_backend(Some(GpuArg::Nvidia), None).unwrap(),
            "nvidia"
        );
        assert_eq!(
            resolve_gpu_backend(Some(GpuArg::Rocm), None).unwrap(),
            "rocm"
        );
        assert!(resolve_gpu_backend(Some(GpuArg::None), None)
            .unwrap()
            .is_empty());
        assert!(resolve_gpu_backend(None, None).unwrap().is_empty());
    }

    #[test]
    /// gpu_backend_config_parses_known_values - config file parsing.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn gpu_backend_config_parses_known_values() {
        assert_eq!(resolve_gpu_backend(None, Some("rocm")).unwrap(), "rocm");
        assert!(resolve_gpu_backend(None, Some("none")).unwrap().is_empty());
        assert_eq!(parse_gpu_arg("ROCm").unwrap(), GpuArg::Rocm);
        assert_eq!(parse_gpu_arg("Auto").unwrap(), GpuArg::Auto);
    }

    #[test]
    /// gpu_backend_config_rejects_unknown_value - config file validation.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn gpu_backend_config_rejects_unknown_value() {
        assert!(parse_gpu_arg("intel").is_err());
        assert!(parse_gpu_arg("").is_err());
    }
}
