use anyhow::{Context, Result};
use clap::Args;
use std::path::{Path, PathBuf};
use xbin_core::detect;
use xbin_core::metadata::{BunFeatures, EmbeddedInterpreter};
use xbin_core::pkgmgr;

#[derive(Args)]
pub struct BuildArgs {
    /// Path to the app directory
    #[arg(default_value = ".")]
    pub app: PathBuf,

    /// Output file path
    #[arg(short, long, default_value = "app.xbin")]
    pub output: PathBuf,

    /// Signing key path
    #[arg(short, long)]
    pub key: Option<PathBuf>,

    /// Isolation mode
    #[arg(long, default_value = "sandbox")]
    pub isolation: String,

    /// Enable seccomp BPF
    #[arg(long)]
    pub seccomp: bool,

    /// Enable encryption
    #[arg(long)]
    pub encrypt: bool,

    /// Use `SquashFS` instead of zstd+tar
    #[arg(long)]
    pub squashfs: bool,

    /// Target architecture
    #[arg(long)]
    pub target: Option<String>,

    /// Skip dependency installation
    #[arg(long)]
    pub no_install: bool,

    /// Environment file to bake in (KEY=VALUE per line)
    #[arg(long)]
    pub env_file: Option<PathBuf>,

    /// Set environment variable (repeatable): --env KEY=VALUE
    #[arg(long = "env", action = clap::ArgAction::Append)]
    pub env: Vec<String>,

    /// Version string
    #[arg(long)]
    pub version_info: Option<String>,

    /// Author name
    #[arg(long)]
    pub author: Option<String>,

    /// Description
    #[arg(long)]
    pub description: Option<String>,

    /// License
    #[arg(long)]
    pub license: Option<String>,

    /// Dry run — show what would be built without building
    #[arg(long)]
    pub dry_run: bool,

    /// Incremental rebuild — reuse unchanged layers from existing .xbin
    #[arg(long)]
    pub update: bool,

    /// Include extra files/directories in the rootfs (repeatable)
    #[arg(long = "include", action = clap::ArgAction::Append)]
    pub include: Vec<PathBuf>,

    /// Enable persistent storage directory (`XBIN_PERSIST_DIR`)
    #[arg(long)]
    pub persist: bool,

    /// Remove unused `node_modules` packages (tree-shaking)
    #[arg(long)]
    pub tree_shake: bool,

    /// Minify JS/TS/CSS files before packaging
    #[arg(long)]
    pub minify: bool,

    /// Health check HTTP port (sets `XBIN_HEALTH_PORT`)
    #[arg(long)]
    pub health_port: Option<u16>,

    /// OpenTelemetry OTLP endpoint (sets `OTEL_EXPORTER_OTLP_ENDPOINT`)
    #[arg(long)]
    pub otel_endpoint: Option<String>,

    /// OpenTelemetry protocol (default: grpc)
    #[arg(long, default_value = "grpc")]
    pub otel_protocol: String,

    /// Scheduled task (repeatable): --cron NAME:SCHEDULE
    #[arg(long = "cron", action = clap::ArgAction::Append)]
    pub cron: Vec<String>,

    /// Force re-detection of dependencies (overwrite `xbin.lock`)
    #[arg(long)]
    pub redetect: bool,

    /// Quiet output — suppress all non-error messages
    #[arg(short, long)]
    pub quiet: bool,

    /// Output build result as JSON to stdout
    #[arg(long)]
    pub json: bool,

    /// Embed interpreter in the binary (python3, node, deno, ruby, php, perl, java, go, wasm, custom)
    #[arg(long)]
    pub embed_interpreter: Option<String>,

    /// Custom path to embedded interpreter (for --embed-interpreter custom)
    #[arg(long)]
    pub interpreter_path: Option<String>,

    /// Enable WASM support with wasmtime
    #[arg(long)]
    pub wasm: bool,

    /// Path to wasmtime binary
    #[arg(long)]
    pub wasmtime_path: Option<PathBuf>,

    /// Cross-compile for target architectures (comma-separated, e.g., aarch64,arm64)
    #[arg(long)]
    pub cross_compile: Option<String>,

    /// Use intelligent build cache (skip extraction if hash matches)
    #[arg(long)]
    pub use_cache: bool,

    /// Clear build cache before building
    #[arg(long)]
    pub clear_cache: bool,

    /// Health check endpoint path (default: /health)
    #[arg(long)]
    pub health_endpoint: Option<String>,
}

pub fn run(args: BuildArgs, verbose: bool) -> Result<()> {
    // Quiet mode overrides verbose
    let verbose = verbose && !args.quiet;

    let app_dir = args
        .app
        .canonicalize()
        .context("failed to canonicalize app path")?;
    if !app_dir.is_dir() {
        anyhow::bail!("{} is not a directory", app_dir.display());
    }

    let output = if args.output.extension().is_some_and(|e| e == "xbin") {
        args.output.clone()
    } else {
        args.output.join("app.xbin")
    };

    // Load .xbin.toml config if present
    let config = load_config(&app_dir);

    // Apply config defaults (CLI flags override)
    let isolation = if args.isolation != "sandbox" {
        args.isolation
    } else {
        config.build.isolation.unwrap_or_else(|| "sandbox".into())
    };

    let seccomp = args.seccomp || config.build.seccomp.unwrap_or(false);
    let encrypt = args.encrypt || config.build.encrypt.unwrap_or(false);
    let squashfs = args.squashfs || config.build.squashfs.unwrap_or(false);
    let version_info = args.version_info.or(config.package.version);
    let author = args.author.or(config.package.author);
    let description = args.description.or(config.package.description);
    let license = args.license.or(config.package.license);
    let env_file = args.env_file.or(config.build.env_file.map(PathBuf::from));
    let no_install = args.no_install || config.build.no_install.unwrap_or(false);
    let target = args.target.or(config.build.target);
    let isolation_num: u32 = isolation.parse().unwrap_or(1);

    // Detect runtime
    let runtime = detect::detect_runtime(&app_dir).context(
        "could not detect runtime — supported: python, node, deno, java, ruby, dotnet, go, php, perl, binary",
    )?;
    let runtime_name = runtime.name();

    eprintln!("Detected runtime: {runtime_name}");

    // Dry run: print plan and exit
    if args.dry_run {
        eprintln!("Dry run — would build:");
        eprintln!("  App:       {}", app_dir.display());
        eprintln!("  Output:    {}", output.display());
        eprintln!("  Runtime:   {runtime_name}");
        eprintln!("  Isolation: {isolation}");
        eprintln!("  Seccomp:   {seccomp}");
        eprintln!("  Encrypt:   {encrypt}");
        eprintln!("  SquashFS:  {squashfs}");
        if let Some(ref v) = version_info {
            eprintln!("  Version:   {v}");
        }
        if let Some(ref a) = author {
            eprintln!("  Author:    {a}");
        }
        if let Some(ref d) = description {
            eprintln!("  Desc:      {d}");
        }
        if let Some(ref l) = license {
            eprintln!("  License:   {l}");
        }
        if let Some(ref t) = target {
            eprintln!("  Target:    {t}");
        }
        if let Some(ref e) = env_file {
            eprintln!("  Env file:  {}", e.display());
        }
        if no_install {
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
        if args.otel_endpoint.is_some() {
            eprintln!("  OTel:      {}", args.otel_endpoint.as_deref().unwrap());
        }
        if !args.cron.is_empty() {
            eprintln!("  Cron:      {} task(s)", args.cron.len());
        }
        if !args.include.is_empty() {
            for inc in &args.include {
                eprintln!("  Include:   {}", inc.display());
            }
        }

        // Detect package managers
        let all_mgrs = pkgmgr::detect_all_pkgmgrs(&app_dir, runtime_name);
        if all_mgrs.is_empty() {
            eprintln!("  Pkg mgr:   (none)");
        } else {
            for mgr in &all_mgrs {
                eprintln!("  Pkg mgr:   {}", mgr.name());
                if !no_install {
                    let cmd = mgr.install_cmd();
                    eprintln!("  Install:   {}", cmd.join(" "));
                }
            }
        }

        // Estimate sizes
        let file_count = count_files(&app_dir);
        eprintln!("  Files:     {file_count}");

        if verbose {
            eprintln!("\nFile tree:");
            print_tree(&app_dir, 2);
        }

        return Ok(());
    }

    // ── Tree-shaking: remove unused node_modules packages ──────────────
    if args.tree_shake {
        let removed = xbin_core::treeshake::prune_node_modules(&app_dir, verbose)
            .context("tree-shaking failed")?;
        if verbose {
            eprintln!("  tree-shake: removed {removed} unused package(s)");
        }
    }

    // ── Minification: shrink JS/TS/CSS ────────────────────────────────
    if args.minify {
        let minified =
            xbin_core::minify::minify_app_dir(&app_dir, verbose).context("minification failed")?;
        if verbose {
            eprintln!("  minify: minified {minified} file(s)");
        }
    }

    // ── Compute hashes for incremental update ──────────────────────────
    let new_app_hash = xbin_core::include::hash_app_files(&app_dir);
    let new_rt_hash = xbin_core::include::hash_lock_file(&app_dir);

    // ── Intelligent build cache: skip rebuild if hash matches ──────────
    if args.use_cache {
        let cache = xbin_core::paths::BuildCache::new(&app_dir, 50);
        if let Some(cached) = cache.find(&new_app_hash, &new_rt_hash) {
            if verbose {
                eprintln!("[xbin] cache hit — reusing cached build");
            }
            std::fs::copy(&cached, &output).context("failed to copy cached .xbin to output")?;
            if args.json {
                let result = serde_json::json!({
                    "output": output.to_string_lossy(),
                    "runtime": runtime_name,
                    "cache_hit": true,
                });
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            return Ok(());
        }
        if verbose {
            eprintln!("[xbin] cache miss — building from scratch");
        }
    }

    if args.clear_cache {
        let cache = xbin_core::paths::BuildCache::new(&app_dir, 50);
        cache.clear().ok();
        if verbose {
            eprintln!("  cache: cleared");
        }
    }

    // ── Incremental update: skip rebuild if nothing changed ────────────
    if args.update && output.exists() {
        if let Some((old_app_hash, old_rt_hash)) = read_existing_hashes(&output) {
            if old_app_hash == new_app_hash && old_rt_hash == new_rt_hash {
                if verbose {
                    eprintln!("[xbin] everything up to date, nothing to rebuild");
                }
                return Ok(());
            } else if old_rt_hash == new_rt_hash && old_app_hash != new_app_hash {
                if verbose {
                    eprintln!("[xbin] app changed, reusing runtime layer (full layer reuse not yet supported in Rust CLI — doing full rebuild)");
                }
            } else if verbose {
                eprintln!("[xbin] runtime deps changed, full rebuild");
            }
        }
    }

    // Detect and install all package managers (primary + secondary)
    let all_pkg_mgrs = pkgmgr::detect_all_pkgmgrs(&app_dir, runtime_name);
    for mgr in &all_pkg_mgrs {
        if verbose {
            eprintln!("Package manager: {}", mgr.name());
        }

        if !no_install {
            let cmd = mgr.install_cmd();

            // Check if the binary exists before trying to run it
            if !is_command_available(cmd[0]) {
                // For node/npm: download static node to temp dir
                if matches!(
                    mgr,
                    pkgmgr::PkgMgr::Npm
                        | pkgmgr::PkgMgr::Pnpm
                        | pkgmgr::PkgMgr::Yarn
                        | pkgmgr::PkgMgr::Bun
                ) {
                    let bin_dir = ensure_node(verbose)?;
                    // Prepend to PATH for this process only
                    let current = std::env::var("PATH").unwrap_or_default();
                    std::env::set_var("PATH", format!("{}:{current}", bin_dir.display()));
                } else {
                    eprintln!(
                        "[xbin] skipping {} — `{}` not found on PATH",
                        mgr.name(),
                        cmd[0]
                    );
                    continue;
                }
            }

            eprintln!("Installing dependencies ({})...", mgr.name());

            // For composer: auto-download composer.phar if not on PATH
            let (prog, extra_args) = if matches!(mgr, pkgmgr::PkgMgr::Composer) {
                ensure_composer(&app_dir, verbose)?
            } else {
                (cmd[0].to_string(), Vec::new())
            };

            let mut full_args: Vec<String> = extra_args;
            full_args.extend(cmd.iter().skip(1).map(|s| s.to_string()));

            let status = std::process::Command::new(&prog)
                .args(&full_args)
                .current_dir(&app_dir)
                .status()
                .context(format!("failed to run `{}` — is it installed?", prog))?;
            if !status.success() {
                eprintln!(
                    "[xbin] warning: {} installation failed (exit code {})",
                    mgr.name(),
                    status.code().unwrap_or(-1)
                );
            }
        }
    }

    // Find stub binary
    let stub = find_stub(&target)?;
    let stub_bytes = std::fs::read(&stub)
        .with_context(|| format!("failed to read stub binary at {}", stub.display()))?;

    // Create temp directory for layer building
    let tmp = tempfile::tempdir().context("failed to create temp directory")?;
    let rootfs = tmp.path().join("rootfs");
    std::fs::create_dir_all(&rootfs).context("failed to create rootfs directory")?;

    // Copy app files
    copy_dir_recursive_with(&app_dir, &rootfs.join("app"), no_install)
        .context("failed to copy app files")?;

    // ── Include extra files ───────────────────────────────────────────
    if !args.include.is_empty() {
        let app_dest = rootfs.join("app");
        let count = xbin_core::include::copy_include_paths(&args.include, &app_dest)
            .context("failed to copy include paths")?;
        if verbose {
            eprintln!("  include: copied {count} path(s) into rootfs");
        }
    }

    // Build deterministic tar (streaming: tar → zstd, no in-memory buffer)
    eprintln!("Creating payload...");
    let t0 = std::time::Instant::now();
    let payload =
        xbin_core::tar::create_tar_zstd(&rootfs).context("failed to create tar+zstd payload")?;
    let compress_ms = t0.elapsed().as_millis();
    if verbose {
        eprintln!(
            "  compress: {compress_ms}ms, {} MB",
            payload.len() as f64 / 1_048_576.0
        );
    }

    // Build metadata
    let app_name = app_dir
        .file_name()
        .map_or_else(|| "app".to_string(), |n| n.to_string_lossy().into());

    let mut env_map = serde_json::Map::new();
    env_map.insert("XBIN_RUNTIME".into(), runtime_name.into());
    env_map.insert("XBIN_APP_NAME".into(), app_name.clone().into());

    // Load env-file (KEY=VALUE per line, # comments, blank lines)
    if let Some(ref ef) = env_file {
        if let Ok(content) = std::fs::read_to_string(ef) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    env_map.insert(k.trim().into(), v.trim().into());
                }
            }
        }
    }

    // Inline --env KEY=VALUE flags (override env-file)
    for entry in &args.env {
        if let Some((k, v)) = entry.split_once('=') {
            env_map.insert(k.trim().into(), v.trim().into());
        }
    }

    // ── Disable Xdebug for PHP apps (avoids "Could not connect to debugging client") ──
    if runtime_name == "php" && !env_map.contains_key("XDEBUG_MODE") {
        env_map.insert(
            "XDEBUG_MODE".into(),
            serde_json::Value::String("off".into()),
        );
        if verbose {
            eprintln!("  xdebug: XDEBUG_MODE=off (auto-injected for PHP)");
        }
    }

    // ── Persistent storage ────────────────────────────────────────────
    if args.persist {
        let persist_dir = xbin_core::persistent::get_persist_dir(&app_name);
        let _ = xbin_core::persistent::ensure_persist_dir(&app_name);
        env_map.insert(
            "XBIN_PERSIST_DIR".into(),
            serde_json::Value::String(persist_dir.to_string_lossy().into()),
        );
        if verbose {
            eprintln!("  persistent storage: {}", persist_dir.display());
        }
    }

    // ── Health check port ─────────────────────────────────────────────
    if let Some(port) = args.health_port {
        env_map.insert(
            "XBIN_HEALTH_PORT".into(),
            serde_json::Value::String(port.to_string()),
        );
        if verbose {
            eprintln!("  health: endpoint enabled on port {port}");
        }
    }

    // ── OpenTelemetry ─────────────────────────────────────────────────
    if let Some(ref endpoint) = args.otel_endpoint {
        let version = version_info.as_deref().unwrap_or("");
        let otel_env = xbin_core::otel::build_otel_env(
            &app_name,
            version,
            endpoint,
            &args.otel_protocol,
            "otlp",
            "otlp",
            "none",
        );
        for (k, v) in &otel_env {
            env_map.insert(k.clone(), serde_json::Value::String(v.clone()));
        }
        if verbose {
            eprintln!(
                "  otel: endpoint={endpoint} protocol={}",
                args.otel_protocol
            );
        }
    }

    // ── Cron/scheduled tasks ──────────────────────────────────────────
    if !args.cron.is_empty() {
        let mut tasks_json: Vec<serde_json::Value> = Vec::new();
        for ct in &args.cron {
            if let Some((name, schedule)) = ct.split_once(':') {
                let interval = xbin_core::cron::parse_schedule(schedule);
                tasks_json.push(serde_json::json!({
                    "name": name,
                    "schedule": schedule,
                    "interval_secs": interval,
                }));
                if verbose {
                    eprintln!("  cron: {name} -> every {interval}s (from {schedule})");
                }
            } else {
                anyhow::bail!("--cron format: NAME:SCHEDULE (got '{ct}')");
            }
        }
        env_map.insert(
            "XBIN_CRON_TASKS".into(),
            serde_json::Value::Array(tasks_json),
        );
        if verbose {
            eprintln!("  cron: {} task(s) registered", args.cron.len());
        }
    }

    let env_pairs: Vec<(String, String)> = env_map
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
        .collect();

    let entrypoint =
        detect::resolve_entrypoint(&app_dir, runtime).unwrap_or_else(|| vec!["run".to_string()]);

    let mut bun_features = BunFeatures::default();

    if let Some(ref interp) = args.embed_interpreter {
        let interpreter = match interp.to_lowercase().as_str() {
            "python3" | "python" => EmbeddedInterpreter::Python3,
            "node" => EmbeddedInterpreter::Node,
            "deno" => EmbeddedInterpreter::Deno,
            "ruby" => EmbeddedInterpreter::Ruby,
            "php" => EmbeddedInterpreter::Php,
            "perl" => EmbeddedInterpreter::Perl,
            "java" => EmbeddedInterpreter::Java,
            "go" => EmbeddedInterpreter::Go,
            "wasm" => EmbeddedInterpreter::Wasm,
            other => EmbeddedInterpreter::Custom(other.to_string()),
        };
        bun_features.embedded_runtime.interpreter = Some(interpreter);
        if let Some(path) = &args.interpreter_path {
            bun_features.embedded_runtime.interpreter_path = Some(path.clone());
        }
        if verbose {
            eprintln!(
                "  embedded runtime: {}",
                bun_features.embedded_runtime.interpreter.as_ref().unwrap()
            );
        }
    }

    if args.wasm {
        bun_features.wasm.enabled = true;
        if let Some(ref path) = args.wasmtime_path {
            bun_features.wasm.wasmtime_path = Some(path.display().to_string());
        }
        if verbose {
            eprintln!("  wasm: enabled");
        }
    }

    if let Some(port) = args.health_port {
        bun_features.health_check.enabled = true;
        bun_features.health_check.port = port;
        if let Some(ref ep) = args.health_endpoint {
            bun_features.health_check.endpoint.clone_from(ep);
        }
        if verbose {
            eprintln!("  health check: port {}", port);
        }
    }

    if let Some(ref cross) = args.cross_compile {
        let targets: Vec<String> = cross.split(',').map(|s| s.trim().to_string()).collect();
        bun_features.cross_compile_targets = targets;
        if verbose {
            eprintln!("  cross-compile: {:?}", bun_features.cross_compile_targets);
        }
    }

    bun_features.build_cache.enabled = args.use_cache;

    bun_features
        .validate()
        .map_err(|e| anyhow::anyhow!("Invalid build options: {}", e))?;

    let meta = xbin_core::assembly::build_meta_json(
        &app_name,
        runtime_name,
        isolation_num,
        &entrypoint,
        &env_pairs,
        &[],
        &xbin_core::assembly::MetaOptions {
            version: version_info,
            author,
            description,
            license,
            payload_format: Some("zstd-tar".to_string()),
            seccomp,
            app_hash: Some(new_app_hash.clone()),
            rt_deps_hash: Some(new_rt_hash.clone()),
        },
        &bun_features,
    )?;

    // Assemble
    eprintln!("Assembling {}...", output.display());

    let size = xbin_core::assembly::assemble_xbin(
        &output,
        &stub_bytes,
        &payload,
        &meta,
        encrypt,
        squashfs,
        target.as_deref(),
    )
    .context("failed to assemble xbin")?;

    eprintln!(
        "Built {} ({:.1}MB)",
        output.display(),
        size as f64 / (1024.0 * 1024.0)
    );

    // ── Store in build cache ──────────────────────────────────────────
    if args.use_cache {
        let cache = xbin_core::paths::BuildCache::new(&app_dir, 50);
        if cache.store(&new_app_hash, &new_rt_hash, &output).is_ok() && verbose {
            eprintln!("  cache: stored build");
        }
    }

    if args.json {
        let result = serde_json::json!({
            "output": output.to_string_lossy(),
            "size_bytes": size,
            "runtime": runtime_name,
            "format": "zstd-tar",
            "signed": args.key.is_some(),
            "encrypted": encrypt,
        });
        println!("{}", serde_json::to_string_pretty(&result)?);
    }

    // Sign if key provided
    if let Some(key_path) = &args.key {
        if verbose {
            eprintln!("Signing...");
        }
        super::sign::sign_file(&output, key_path, !verbose)?;
    }

    Ok(())
}

#[derive(Default, serde::Deserialize)]
struct XbinConfig {
    #[serde(default)]
    build: BuildConfig,
    #[serde(default)]
    package: PackageConfig,
}

#[derive(Default, serde::Deserialize)]
struct BuildConfig {
    pub isolation: Option<String>,
    pub seccomp: Option<bool>,
    pub encrypt: Option<bool>,
    pub squashfs: Option<bool>,
    pub target: Option<String>,
    pub no_install: Option<bool>,
    pub env_file: Option<String>,
}

#[derive(Default, serde::Deserialize)]
struct PackageConfig {
    pub version: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
}

fn load_config(app_dir: &Path) -> XbinConfig {
    let config_path = app_dir.join(".xbin.toml");
    if !config_path.exists() {
        return XbinConfig::default();
    }
    match std::fs::read_to_string(&config_path) {
        Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
            eprintln!("[xbin] warning: invalid .xbin.toml: {e}");
            XbinConfig::default()
        }),
        Err(_) => XbinConfig::default(),
    }
}

/// Check if a command is available on PATH.
fn is_command_available(name: &str) -> bool {
    std::process::Command::new("sh")
        .args(["-c", &format!("command -v {name}")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Ensure node + npm are available for the build.
/// Downloads a static node to `/tmp/xbin-build-tools/node/` if not on PATH.
/// Does NOT pollute the user's system PATH.
fn ensure_node(verbose: bool) -> Result<PathBuf> {
    let tools_dir = PathBuf::from("/tmp/xbin-build-tools/node");
    let node_bin = tools_dir.join("bin/node");
    let npm_bin = tools_dir.join("bin/npm");

    // Already downloaded
    if node_bin.exists() && npm_bin.exists() {
        if verbose {
            eprintln!("  using cached node from {}", tools_dir.display());
        }
        return Ok(tools_dir.join("bin"));
    }

    if verbose {
        eprintln!("  downloading node to {}...", tools_dir.display());
    }

    std::fs::create_dir_all(&tools_dir).context("failed to create build tools directory")?;

    // Detect arch
    let arch = std::env::consts::ARCH;
    let node_arch = match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => arch,
    };
    let node_os = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "darwin",
        other => other,
    };

    // Get latest LTS version
    let version_output = std::process::Command::new("sh")
        .args([
            "-c",
            "curl -sL https://nodejs.org/dist/index.json | head -20 | grep -o '\"v[0-9.]*\"' | head -1 | tr -d '\"v'",
        ])
        .output()
        .context("failed to query node.js versions")?;
    let version = String::from_utf8_lossy(&version_output.stdout)
        .trim()
        .to_string();
    if version.is_empty() {
        anyhow::bail!("node.js not found and auto-download failed — install: https://nodejs.org");
    }

    let tarball = format!("node-v{version}-{node_os}-{node_arch}.tar.xz");
    let url = format!("https://nodejs.org/dist/v{version}/{tarball}");

    if verbose {
        eprintln!("  downloading node v{version} ({node_os}-{node_arch})...");
    }

    let status = std::process::Command::new("sh")
        .args([
            "-c",
            &format!(
                "curl -sL {url} | tar xJ --strip-components=1 -C {}",
                tools_dir.display()
            ),
        ])
        .status()
        .context("failed to download node.js")?;

    if !status.success() || !node_bin.exists() {
        anyhow::bail!("failed to download node.js — install manually: https://nodejs.org");
    }

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&node_bin, std::fs::Permissions::from_mode(0o755)).ok();
        std::fs::set_permissions(&npm_bin, std::fs::Permissions::from_mode(0o755)).ok();
    }

    if verbose {
        eprintln!("  node v{version} ready at {}", tools_dir.display());
    }

    Ok(tools_dir.join("bin"))
}

/// Returns `(program, extra_args)` to prepend to the install command.
fn ensure_composer(app_dir: &Path, verbose: bool) -> Result<(String, Vec<String>)> {
    // Try system composer first
    if std::process::Command::new("composer")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
    {
        return Ok(("composer".into(), Vec::new()));
    }

    // Try php with system composer.phar
    if std::process::Command::new("php")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
    {
        let phar = app_dir.join("composer.phar");
        if !phar.exists() {
            if verbose {
                eprintln!("  downloading composer.phar...");
            }
            let status = std::process::Command::new("php")
                .args([
                    "-r",
                    "copy('https://getcomposer.org/download/latest-stable/composer.phar', 'composer.phar');",
                ])
                .current_dir(app_dir)
                .status()
                .context("failed to download composer.phar")?;
            if !status.success() {
                anyhow::bail!(
                    "composer not found and failed to download composer.phar — \
                     install composer: https://getcomposer.org/download"
                );
            }
            // Make it executable
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&phar, std::fs::Permissions::from_mode(0o755)).ok();
            }
        }
        return Ok(("php".into(), vec![phar.to_string_lossy().to_string()]));
    }

    anyhow::bail!(
        "composer not found — install it: https://getcomposer.org/download \
         or install php + composer"
    )
}

fn find_stub(target_arch: &Option<String>) -> Result<PathBuf> {
    if let Ok(path) = std::env::var("XBIN_STUB_PATH") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    let target_dir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into());

    let arch_suffix = match target_arch.as_deref() {
        Some("aarch64") => "aarch64-unknown-linux-musl",
        _ => "x86_64-unknown-linux-musl",
    };

    let stub_name = "xbin-stub";
    let candidates = [
        PathBuf::from(&target_dir)
            .join(arch_suffix)
            .join("release")
            .join(stub_name),
        PathBuf::from("/tmp/xbin-stub-target")
            .join(arch_suffix)
            .join("release")
            .join(stub_name),
        PathBuf::from("stub/target")
            .join(arch_suffix)
            .join("release")
            .join(stub_name),
        PathBuf::from("/usr/local/bin/xbin-stub"),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            return Ok(candidate.clone());
        }
    }

    if let Ok(p) = which::which("xbin-stub") {
        return Ok(p);
    }

    anyhow::bail!("xbin-stub not found — run: make stub")
}

/// Read `app_hash` and `rt_deps_hash` from an existing `.xbin` file's metadata.
fn read_existing_hashes(xbin_path: &Path) -> Option<(String, String)> {
    use xbin_core::format::Footer;

    let mut f = std::fs::File::open(xbin_path).ok()?;
    let footer = Footer::read_from(&mut f).ok()?;
    let meta_size = footer.meta_size.try_into().ok()?;
    let meta_bytes = xbin_core::format::read_at(&mut f, footer.meta_offset, meta_size).ok()?;
    let meta: serde_json::Value = serde_json::from_slice(&meta_bytes).ok()?;
    let app_hash = meta.get("app_hash")?.as_str()?.to_string();
    let rt_hash = meta.get("rt_deps_hash")?.as_str()?.to_string();
    Some((app_hash, rt_hash))
}

fn count_files(dir: &Path) -> usize {
    let mut count = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str == ".git"
                || name_str == "node_modules"
                || name_str == "__pycache__"
                || name_str == ".venv"
                || name_str == "venv"
                || name_str == ".xbin"
            {
                continue;
            }
            if entry.path().is_dir() {
                count += count_files(&entry.path());
            } else {
                count += 1;
            }
        }
    }
    count
}

fn print_tree(dir: &Path, indent: usize) {
    let prefix = " ".repeat(indent);
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str == ".git"
                || name_str == "node_modules"
                || name_str == "__pycache__"
                || name_str == ".venv"
                || name_str == "venv"
                || name_str == ".xbin"
            {
                continue;
            }
            if entry.path().is_dir() {
                eprintln!("{prefix}{name_str}/");
                print_tree(&entry.path(), indent + 2);
            } else {
                eprintln!("{prefix}{name_str}");
            }
        }
    }
}

fn copy_dir_recursive_with(src: &Path, dst: &Path, include_node_modules: bool) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        let name = src_path.file_name().unwrap_or_default().to_string_lossy();
        if name == ".git"
            || name == "__pycache__"
            || name == ".venv"
            || name == "venv"
            || name == ".xbin"
            || (name == "node_modules" && !include_node_modules)
        {
            continue;
        }

        if src_path.is_dir() {
            copy_dir_recursive_with(&src_path, &dst_path, include_node_modules)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
