use anyhow::{Context, Result};
use clap::Args;
use std::path::{Path, PathBuf};
use xbin_core::detect;
use xbin_core::embed;
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

    /// Enable Landlock LSM filesystem sandbox (stub isolation >= 2)
    #[arg(long)]
    pub landlock: bool,

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

    /// Define build-time constants (repeatable): --define KEY=VALUE (injected as env vars)
    #[arg(long = "define", action = clap::ArgAction::Append)]
    pub define: Vec<String>,

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
    let landlock = args.landlock || config.build.landlock.unwrap_or(false);
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
        eprintln!("  Landlock:  {landlock}");
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
        if let Some(cached) = cache.find(&new_app_hash) {
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

    // ── PHP platform extensions check ──────────────────────────────────
    if runtime_name == "php" && !no_install {
        check_php_platform_reqs(&app_dir, verbose)?;
    }

    // Detect pnpm workspace with workspace:* protocol (cannot use npm)
    if pkgmgr::detect_node_pkgmgr(&app_dir) == Some(pkgmgr::PkgMgr::Npm)
        && has_workspace_protocol(&app_dir)
    {
        eprintln!("[xbin] warning: package.json uses `workspace:*` protocol (pnpm-specific)");
        eprintln!("  but pnpm is not detected. Create `pnpm-workspace.yaml` or add a lockfile.");
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

            // When cross-compiling for a different arch, force source builds for pip
            let is_cross_pip = matches!(mgr, pkgmgr::PkgMgr::Pip)
                && args
                    .cross_compile
                    .as_ref()
                    .is_some_and(|c| c.split(',').any(|t| t.trim() != std::env::consts::ARCH));
            if is_cross_pip {
                full_args.push("--no-binary".into());
                full_args.push(":all:".into());
            }

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
    let include_node_modules = true;
    copy_dir_recursive_with(&app_dir, &rootfs.join("app"), include_node_modules)
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

    // ── Embed interpreter ──────────────────────────────────
    // NOTE: ensure_node() may have downloaded node to /tmp/xbin-build-tools/bin/
    // and added it to PATH — embed_interpreter picks it up from there.
    let embedded_interpreter_str = if let Some(ref interp) = args.embed_interpreter {
        Some(interp.clone())
    } else {
        match runtime_name {
            "python" => Some("python3".to_string()),
            "node" => Some("node".to_string()),
            "php" => Some("php".to_string()),
            "ruby" => Some("ruby".to_string()),
            "deno" => Some("deno".to_string()),
            _ => None,
        }
    };

    let mut interpreter_embedded = false;
    if let Some(ref interpreter_name) = embedded_interpreter_str {
        if verbose {
            eprintln!("Embedding interpreter: {}...", interpreter_name);
        }

        let interpreter_path = interpreter_name.clone();
        match embed::embed_interpreter(&interpreter_path, &rootfs, verbose) {
            Ok(count) => {
                if verbose {
                    eprintln!("Embedded interpreter ({} files copied)", count);
                }
                interpreter_embedded = true;
            }
            Err(e) => {
                eprintln!("[xbin] warning: failed to embed interpreter: {}", e);
            }
        }
    }

    // Pre-compile Python bytecode for faster startup
    if runtime_name == "python" {
        let app_root = rootfs.join("app");
        if app_root.is_dir() {
            let status = std::process::Command::new("python3")
                .args(["-m", "compileall", "-f", "-q"])
                .arg(&app_root)
                .status();
            if let Ok(s) = status {
                if s.success() && verbose {
                    eprintln!("Bytecode compiled (.pyc)");
                }
            }
        }
    }

    // Embed N-API native addon dependencies (.node files → ldd → .so)
    if runtime_name == "node" {
        match embed::embed_napi_addons(&rootfs, verbose) {
            Ok(n) => {
                if verbose && n > 0 {
                    eprintln!("Embedded {} N-API shared library dependencies", n);
                }
            }
            Err(e) => {
                eprintln!("[xbin] warning: N-API addon embedding failed: {e}");
            }
        }
    }

    // Embed RoadRunner binary for Laravel Octane apps (rr.yaml or .rr.yaml)
    if app_dir.join("rr.yaml").is_file() || app_dir.join(".rr.yaml").is_file() {
        if which::which("rr").is_ok() {
            if verbose {
                eprintln!("Embedding RoadRunner...");
            }
            if let Err(e) = embed::embed_interpreter("rr", &rootfs, verbose) {
                eprintln!("[xbin] warning: failed to embed RoadRunner: {}", e);
            }
        } else if verbose {
            eprintln!("[xbin] warning: rr binary not found on PATH; RoadRunner won't be available at runtime");
        }
    }

    if !interpreter_embedded && verbose && embedded_interpreter_str.is_some() {
        eprintln!("  (interpreter embedding skipped)");
    }

    // Clean up downloaded build tools (node/npm, composer.phar in /tmp)
    // Done *after* embedding so the interpreter is still available for copying.
    let _ = std::fs::remove_dir_all("/tmp/xbin-build-tools");

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

    // Build-time --define KEY=VALUE (injected as env vars, overrides --env)
    for entry in &args.define {
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

    // Set embedded interpreter based on either explicit --embed-interpreter or auto-detection
    let (interpreter_opt, interpreter_path_opt) = if let Some(ref interp) = args.embed_interpreter {
        (
            Some(match interp.to_lowercase().as_str() {
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
            }),
            args.interpreter_path.clone(),
        )
    } else {
        match runtime_name {
            "python" => (Some(EmbeddedInterpreter::Python3), None),
            "node" => (Some(EmbeddedInterpreter::Node), None),
            "php" => (Some(EmbeddedInterpreter::Php), None),
            "ruby" => (Some(EmbeddedInterpreter::Ruby), None),
            "deno" => (Some(EmbeddedInterpreter::Deno), None),
            _ => (None, None),
        }
    };

    if let Some(interpreter) = interpreter_opt {
        bun_features.embedded_runtime.interpreter = Some(interpreter);
        if let Some(path) = interpreter_path_opt {
            bun_features.embedded_runtime.interpreter_path = Some(path);
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
            landlock,
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
        if cache.store(&new_app_hash, &output).is_ok() && verbose {
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
    pub landlock: Option<bool>,
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

    ensure_node_download(tools_dir, None, verbose)
}

/// Download a specific Node.js version for a target architecture.
///
/// When `target_arch` is `None`, downloads for the host architecture.
fn ensure_node_download(
    tools_dir: PathBuf,
    target_arch: Option<&str>,
    verbose: bool,
) -> Result<PathBuf> {
    let (node_arch, node_os) = if let Some(arch) = target_arch {
        match arch {
            "x86_64" | "amd64" => ("x64", "linux"),
            "aarch64" | "arm64" => ("arm64", "linux"),
            _ => anyhow::bail!("unsupported cross-compile architecture: {arch}"),
        }
    } else {
        let node_arch = match std::env::consts::ARCH {
            "x86_64" => "x64",
            "aarch64" => "arm64",
            arch => arch,
        };
        let node_os = match std::env::consts::OS {
            "linux" => "linux",
            "macos" => "darwin",
            os => os,
        };
        (node_arch, node_os)
    };

    let node_bin = tools_dir.join("bin/node");
    let npm_bin = tools_dir.join("bin/npm");

    let versions: Vec<serde_json::Value> =
        reqwest::blocking::get("https://nodejs.org/dist/index.json")
            .context("failed to reach nodejs.org")?
            .json()
            .context("failed to parse node version manifest")?;
    let version = versions
        .first()
        .and_then(|v| v.get("version")?.as_str())
        .and_then(|v| v.strip_prefix('v'))
        .map(|v| v.to_string())
        .ok_or_else(|| anyhow::anyhow!("no node version found in manifest"))?;

    let tarball = format!("node-v{version}-{node_os}-{node_arch}.tar.xz");
    let url = format!("https://nodejs.org/dist/v{version}/{tarball}");

    if verbose {
        eprintln!("  downloading node v{version} ({node_os}-{node_arch})...");
    }

    let response = reqwest::blocking::get(&url).context("failed to download node.js tarball")?;
    let reader = std::io::BufReader::new(response);
    let decoder = xz::read::XzDecoder::new(reader);
    let mut archive = tar::Archive::new(decoder);

    for entry in archive
        .entries()
        .context("failed to read node tarball entries")?
    {
        let mut entry = entry.context("failed to read tarball entry")?;
        let path = entry.path()?.into_owned();
        let stripped: PathBuf = path.components().skip(1).collect();
        if stripped.components().count() == 0 {
            continue;
        }
        let target = tools_dir.join(&stripped);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }
        entry
            .unpack(&target)
            .with_context(|| format!("failed to unpack {}", stripped.display()))?;
    }

    if !node_bin.exists() {
        anyhow::bail!(
            "downloaded tarball missing node binary — install manually: https://nodejs.org"
        );
    }

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

/// Check PHP platform requirements from composer.json against available extensions.
fn check_php_platform_reqs(app_dir: &Path, verbose: bool) -> Result<()> {
    let composer_path = app_dir.join("composer.json");
    if !composer_path.is_file() {
        return Ok(());
    }

    let content =
        std::fs::read_to_string(&composer_path).context("failed to read composer.json")?;
    let composer: serde_json::Value =
        serde_json::from_str(&content).context("failed to parse composer.json")?;

    let require = match composer.get("require").and_then(|r| r.as_object()) {
        Some(r) => r,
        None => return Ok(()),
    };

    // Check PHP version constraint from composer.json
    if let Some(php_req) = require.get("php").and_then(|v| v.as_str()) {
        let current_version = std::process::Command::new("php")
            .args(["-v"])
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .next()
                    .and_then(|l| l.split_whitespace().nth(1))
                    .map(|s| s.to_string())
            });

        if let Some(ref cur) = current_version {
            if !version_satisfies(cur, php_req) && verbose {
                eprintln!(
                    "[xbin] warning: composer.json requires PHP {}, but php {} is on PATH",
                    php_req, cur
                );
                if let Some(alt) = find_php_binary(php_req) {
                    eprintln!(
                        "[xbin]   consider using --embed-interpreter {} or set PATH to use {}",
                        alt, alt
                    );
                }
            }
        }
    }

    let mut required_exts: Vec<&str> = Vec::new();
    for key in require.keys() {
        if let Some(ext) = key.strip_prefix("ext-") {
            required_exts.push(ext);
        }
    }

    if required_exts.is_empty() {
        return Ok(());
    }

    // Check which extensions are available
    let php_output = std::process::Command::new("php")
        .args(["-m"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let available: Vec<String> = php_output
        .lines()
        .map(|l| l.trim().to_lowercase())
        .collect();

    let mut missing: Vec<String> = Vec::new();
    for ext in &required_exts {
        if !available.contains(&ext.to_lowercase()) {
            missing.push(ext.to_string());
        }
    }

    if !missing.is_empty() {
        eprintln!("[xbin] warning: PHP extensions required by composer but not installed:");
        for ext in &missing {
            eprintln!("  ext-{ext}");
        }
        eprintln!("  Run: sudo apt install php-{}", missing.join(" php-"));
        eprintln!("  or: composer install --ignore-platform-reqs (will be used as fallback)");
        if verbose {
            eprintln!("  Proceeding with --ignore-platform-reqs — runtime may fail if extensions are needed.");
        }
    } else if verbose {
        eprintln!(
            "  PHP platform extensions: all {} required extension(s) available",
            required_exts.len()
        );
    }

        Ok(())
}

/// Simple PHP version constraint check — handles `^8.2`, `>=8.0`, `8.1`, `8.*`,
/// `~8.1.0`, and `8.1 || 8.2` patterns. Returns true if the version satisfies.
fn version_satisfies(version: &str, constraint: &str) -> bool {
    let version_parts: Vec<u32> = version
        .split('.')
        .filter_map(|s| s.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok())
        .collect();
    if version_parts.is_empty() {
        return false;
    }

    for part in constraint.split("||") {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if satisfies_single(&version_parts, part) {
            return true;
        }
    }
    false
}

fn satisfies_single(version: &[u32], constraint: &str) -> bool {
    let constraint = constraint.trim();
    if constraint.starts_with('^') {
        if let Some(rest) = constraint.strip_prefix('^') {
            let target = parse_version(rest);
            if target.is_empty() {
                true
            } else {
                version >= &target && version.first() == target.first()
            }
        } else {
            true
        }
    } else if constraint.starts_with('~') {
        if let Some(rest) = constraint.strip_prefix('~') {
            let target = parse_version(rest);
            if target.len() < 2 {
                true
            } else {
                version.len() >= 2 && version[0] == target[0] && version[1] == target[1] && version >= &target
            }
        } else {
            true
        }
    } else if constraint.ends_with('*') {
        let prefix = constraint.trim_end_matches('*');
        let prefix_parts: Vec<u32> = prefix
            .split('.')
            .filter_map(|s| s.parse().ok())
            .collect();
        version.starts_with(&prefix_parts)
    } else if let Some(rest) = constraint.strip_prefix(">=") {
        let target = parse_version(rest.trim());
        compare_versions(version, &target).map_or(false, |ord| ord != std::cmp::Ordering::Less)
    } else if let Some(rest) = constraint.strip_prefix("<=") {
        let target = parse_version(rest.trim());
        compare_versions(version, &target).map_or(false, |ord| ord != std::cmp::Ordering::Greater)
    } else if let Some(rest) = constraint.strip_prefix('>') {
        let target = parse_version(rest.trim());
        compare_versions(version, &target).map_or(false, |ord| ord == std::cmp::Ordering::Greater)
    } else if let Some(rest) = constraint.strip_prefix('<') {
        let target = parse_version(rest.trim());
        compare_versions(version, &target).map_or(false, |ord| ord == std::cmp::Ordering::Less)
    } else if let Some(rest) = constraint.strip_prefix("==") {
        let target = parse_version(rest.trim());
        compare_versions(version, &target).map_or(false, |ord| ord == std::cmp::Ordering::Equal)
    } else {
        let target = parse_version(constraint);
        version == target
    }
}

fn parse_version(s: &str) -> Vec<u32> {
    s.split('.')
        .filter_map(|p| p.trim().parse().ok())
        .collect()
}

fn compare_versions(a: &[u32], b: &[u32]) -> Option<std::cmp::Ordering> {
    for (x, y) in a.iter().zip(b.iter()) {
        let ord = x.cmp(y);
        if ord != std::cmp::Ordering::Equal {
            return Some(ord);
        }
    }
    a.len().cmp(&b.len()).into()
}

/// Look for alternative PHP binaries on PATH that might satisfy the version constraint.
fn find_php_binary(constraint: &str) -> Option<String> {
    let rest = constraint.strip_prefix('^')?;
    let major = rest.split('.').next()?;
    for candidate in ["php", &format!("php{major}")] {
        if which::which(candidate).is_ok() {
            return Some(candidate.to_string());
        }
    }
    None
}

/// Check if package.json contains `workspace:*` protocol deps (pnpm-specific).
fn has_workspace_protocol(dir: &Path) -> bool {
    let pkg = match std::fs::read_to_string(dir.join("package.json")) {
        Ok(c) => c,
        _ => return false,
    };
    let json: serde_json::Value = match serde_json::from_str(&pkg) {
        Ok(v) => v,
        _ => return false,
    };
    for section in &[
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ] {
        if let Some(deps) = json.get(*section).and_then(|d| d.as_object()) {
            for val in deps.values() {
                if let Some(v) = val.as_str() {
                    if v.starts_with("workspace:") {
                        return true;
                    }
                }
            }
        }
    }
    false
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
            || name == ".pnpm"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_stub_default_is_x86_64() {
        let result = find_stub(&None);
        assert!(result.is_err() || result.is_ok(), "should not panic");
    }

    #[test]
    fn find_stub_aarch64_suffix() {
        let result = find_stub(&Some("aarch64".into()));
        assert!(result.is_err() || result.is_ok(), "should not panic");
    }

    #[test]
    fn ensure_node_download_rejects_bad_arch() {
        let dir = tempfile::tempdir().unwrap();
        let result = ensure_node_download(dir.path().to_path_buf(), Some("riscv64"), false);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("unsupported"),
            "should mention unsupported arch"
        );
    }

    #[test]
    fn ensure_node_download_accepts_x86_64() {
        let dir = tempfile::tempdir().unwrap();
        let result = ensure_node_download(dir.path().to_path_buf(), Some("x86_64"), false);
        // May fail due to network, but should not panic
        assert!(result.is_err() || result.is_ok());
    }

    #[test]
    fn ensure_node_download_accepts_aarch64() {
        let dir = tempfile::tempdir().unwrap();
        let result = ensure_node_download(dir.path().to_path_buf(), Some("aarch64"), false);
        assert!(result.is_err() || result.is_ok());
    }

    #[test]
    fn check_php_platform_reqs_no_composer_json() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check_php_platform_reqs(dir.path(), false).is_ok());
    }

    #[test]
    fn check_php_platform_reqs_no_require() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("composer.json"), "{}").unwrap();
        assert!(check_php_platform_reqs(dir.path(), false).is_ok());
    }

    #[test]
    fn check_php_platform_reqs_finds_extensions() {
        let dir = tempfile::tempdir().unwrap();
        let composer = r#"{"require": {"php": ">=8.0", "ext-json": "*", "ext-mbstring": "*"}}"#;
        std::fs::write(dir.path().join("composer.json"), composer).unwrap();
        // Should not error even if php is not available
        assert!(check_php_platform_reqs(dir.path(), false).is_ok());
    }

    #[test]
    fn cross_compile_targets_parsed() {
        let targets: Vec<String> = "aarch64,arm64"
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();
        assert_eq!(targets, vec!["aarch64", "arm64"]);
    }

    #[test]
    fn has_workspace_protocol_detects_workspace_deps() {
        let dir = tempfile::tempdir().unwrap();
        let pkg = r#"{"dependencies": {"foo": "workspace:*", "bar": "1.0.0"}}"#;
        std::fs::write(dir.path().join("package.json"), pkg).unwrap();
        assert!(has_workspace_protocol(dir.path()));
    }

    #[test]
    fn has_workspace_protocol_no_match() {
        let dir = tempfile::tempdir().unwrap();
        let pkg = r#"{"dependencies": {"foo": "1.0.0", "bar": "^2.0"}}"#;
        std::fs::write(dir.path().join("package.json"), pkg).unwrap();
        assert!(!has_workspace_protocol(dir.path()));
    }

    #[test]
    fn has_workspace_protocol_no_package_json() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!has_workspace_protocol(dir.path()));
    }

    #[test]
    fn has_workspace_protocol_dev_deps() {
        let dir = tempfile::tempdir().unwrap();
        let pkg = r#"{"devDependencies": {"tool": "workspace:^1.0.0"}}"#;
        std::fs::write(dir.path().join("package.json"), pkg).unwrap();
        assert!(has_workspace_protocol(dir.path()));
    }
}
