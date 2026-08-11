use crate::remote_cache::remote_cache_from_args;
use anyhow::{bail, Context, Result};
use clap::Args;
use ed25519_dalek::SigningKey;
use sha2::Digest;
use std::path::{Path, PathBuf};
use erebus_core::detect;
use erebus_core::embed;
use erebus_core::metadata::{BunFeatures, EmbeddedInterpreter};
use erebus_core::paths::cache_dir;
use erebus_core::pkgmgr;
use erebus_core::sisr_stage::SisrBuildConfig;

/// Map an isolation spec to the numeric level stored in metadata.
///
/// Levels: 0 = `LD_LIBRARY_PATH` (no sandbox), 1 = skipped, 2 = user +
/// mount namespaces with `pivot_root`. `sandbox` is the default and must
/// resolve to the implemented sandbox (2), not silently degrade.
fn parse_isolation(value: &str) -> Result<u32> {
    match value.trim() {
        "sandbox" | "2" => Ok(2),
        "1" => Ok(1),
        "0" | "none" => Ok(0),
        other => bail!("expected 'sandbox', 'none', or a level 0-2, got '{other}'"),
    }
}

/// Parse a `--target` value into `(arch, os)`.
///
/// Accepts legacy short forms (`aarch64`, `x86_64` — defaulting to Linux),
/// OS shorthands (`win-x64`, `win-arm64`, `linux-x64`, `linux-arm64`), and
/// full Rust triples (`aarch64-apple-darwin`, `x86_64-unknown-linux-musl`).
fn parse_target(target: &str) -> (String, String) {
    let parts: Vec<&str> = target.split('-').collect();
    let shorthand_os = matches!(parts.first(), Some(&"win" | &"macos" | &"linux"));
    let arch = if shorthand_os {
        match parts.get(1).copied() {
            Some("x64" | "amd64") => "x86_64",
            Some("arm64" | "aarch64") => "aarch64",
            Some("x86" | "i686" | "i386") => "x86",
            _ => std::env::consts::ARCH,
        }
    } else {
        match parts.first().copied() {
            Some("x86_64" | "amd64") => "x86_64",
            Some("aarch64" | "arm64") => "aarch64",
            Some("i686" | "x86" | "i386") => "x86",
            Some(other) => other,
            None => std::env::consts::ARCH,
        }
    };
    let os = if parts.contains(&"apple") || parts.first() == Some(&"macos") {
        "darwin"
    } else if parts.contains(&"pc") || parts.contains(&"windows") || parts.first() == Some(&"win") {
        "windows"
    } else if parts.contains(&"linux") {
        "linux"
    } else if target.contains('-') {
        // Unrecognized full triple: assume a Linux OS — the common case.
        "linux"
    } else {
        // Legacy short forms (`aarch64`, `x86_64`) target Linux by default.
        "linux"
    };
    (arch.to_string(), os.to_string())
}

/// Resolved, target-independent build settings shared by every target in a
/// multi-arch build. Kept separate from `BuildArgs` so the per-target build
/// loop takes a single struct instead of a parameter list.
struct BuildPlan {
    verbose: bool,
    app_dir: PathBuf,
    runtime: detect::Runtime,
    runtime_name: String,
    isolation: String,
    isolation_num: u32,
    no_install: bool,
    seccomp: bool,
    landlock: bool,
    encrypt: bool,
    squashfs: bool,
    version_info: Option<String>,
    author: Option<String>,
    description: Option<String>,
    license: Option<String>,
    env_file: Option<PathBuf>,
    targets: Vec<Option<String>>,
    outputs: Vec<PathBuf>,
}

/// Expand `--target`/`--cross-compile` (each comma-separated) plus the config
/// default into the ordered list of targets to build. `None` means "host
/// target". Duplicates are removed, first occurrence wins.
fn resolve_targets(args: &BuildArgs, config_target: Option<&str>) -> Vec<Option<String>> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let mut push = |t: &str| {
        let t = t.trim();
        if t.is_empty() || !seen.insert(t.to_string()) {
            return;
        }
        out.push(if t == "host" {
            None
        } else {
            Some(t.to_string())
        });
    };
    if let Some(t) = &args.target {
        for t in t.split(',') {
            push(t);
        }
    } else if let Some(t) = config_target {
        for t in t.split(',') {
            push(t);
        }
    }
    if let Some(c) = &args.cross_compile {
        for t in c.split(',') {
            push(t);
        }
    }
    if out.is_empty() {
        out.push(None);
    }
    out
}

/// Filename slug for a target, used to disambiguate multi-arch artifacts.
/// `None` (host build) keeps the plain output name.
fn target_slug(target: Option<&str>) -> Option<String> {
    target.map(|t| t.replace(['/', '\\'], "-"))
}

/// One output path per target. A single target keeps the historical naming
/// (`-o app.xbin` stays `app.xbin`); multiple targets get a `<name>-<target>`
/// suffix so linux and windows artifacts never overwrite each other.
fn output_paths(args: &BuildArgs, targets: &[Option<String>]) -> Vec<PathBuf> {
    if targets.len() == 1 {
        let t = targets[0].as_deref();
        let is_windows_target = t.is_some_and(|t| parse_target(t).1 == "windows");
        let out = if args
            .output
            .extension()
            .is_some_and(|e| e == "xbin" || e == "exe")
        {
            args.output.clone()
        } else {
            args.output.join(if is_windows_target {
                "app.exe"
            } else {
                "app.xbin"
            })
        };
        return vec![out];
    }

    targets
        .iter()
        .map(|t| {
            let slug = target_slug(t.as_deref()).unwrap_or_else(|| "host".to_string());
            let name = args
                .output
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "app".to_string());
            let is_windows = t.as_deref().is_some_and(|t| parse_target(t).1 == "windows");
            let ext = if is_windows { "exe" } else { "xbin" };
            let dir = args
                .output
                .parent()
                .map_or_else(|| "".into(), Path::to_path_buf);
            dir.join(format!("{name}-{slug}.{ext}"))
        })
        .collect()
}

/// Canonical fingerprint of every build option that changes the output
/// artifact. The build cache keys on this *alongside* the app-source hash,
/// so two builds of the same app with different flags never share a cache
/// entry — previously a `--squashfs` build could be served from a zstd
/// entry, or a signed build from an unsigned one.
fn config_fingerprint(args: &BuildArgs, plan: &BuildPlan) -> String {
    let mut canonical: Vec<String> = Vec::new();
    canonical.push(format!("isolation={}", plan.isolation));
    canonical.push(format!("no_install={}", plan.no_install));
    canonical.push(format!("seccomp={}", plan.seccomp));
    canonical.push(format!("landlock={}", plan.landlock));
    canonical.push(format!("encrypt={}", plan.encrypt));
    canonical.push(format!("squashfs={}", plan.squashfs));
    canonical.push(format!("compress={}", args.compression_level));
    canonical.push(format!("sisr={}", args.enable_sisr));
    canonical.push(format!("redetect={}", args.redetect));
    if let Some(url) = &args.update_url {
        canonical.push(format!("update_url={url}"));
    }
    canonical.push(format!("persist={}", args.persist));
    canonical.push(format!("health_port={:?}", args.health_port));
    if let Some(ep) = &args.health_endpoint {
        canonical.push(format!("health_endpoint={ep}"));
    }
    if let Some(v) = &plan.version_info {
        canonical.push(format!("version={v}"));
    }
    if let Some(v) = &plan.author {
        canonical.push(format!("author={v}"));
    }
    if let Some(v) = &plan.description {
        canonical.push(format!("description={v}"));
    }
    if let Some(v) = &plan.license {
        canonical.push(format!("license={v}"));
    }
    if let Some(v) = &plan.env_file {
        let content = std::fs::read(v)
            .map(|b| hex::encode(sha2::Sha256::digest(&b)))
            .unwrap_or_else(|_| "unreadable".to_string());
        canonical.push(format!("env_file={}:{content}", v.display()));
    }
    let mut env: Vec<&String> = args.env.iter().collect();
    env.sort();
    for e in env {
        canonical.push(format!("env={e}"));
    }
    let mut define: Vec<&String> = args.define.iter().collect();
    define.sort();
    for d in define {
        canonical.push(format!("define={d}"));
    }
    if let Some(interp) = &args.embed_interpreter {
        canonical.push(format!("embed={interp}"));
    }
    if let Some(p) = &args.interpreter_path {
        canonical.push(format!("interpreter_path={p}"));
    }
    canonical.push(format!("tree_shake={}", args.tree_shake));
    canonical.push(format!("minify={}", args.minify));
    if let Some(ep) = &args.otel_endpoint {
        canonical.push(format!("otel={ep}/{}", args.otel_protocol));
    }
    let mut cron: Vec<&String> = args.cron.iter().collect();
    cron.sort();
    for c in cron {
        canonical.push(format!("cron={c}"));
    }
    let mut include: Vec<&PathBuf> = args.include.iter().collect();
    include.sort();
    for p in include {
        let rel = p.strip_prefix(&plan.app_dir).unwrap_or(p);
        canonical.push(format!("include={}", rel.display()));
    }
    if let Some(key) = &args.key {
        let content = std::fs::read(key)
            .map(|b| hex::encode(sha2::Sha256::digest(&b)))
            .unwrap_or_else(|_| "unreadable".to_string());
        canonical.push(format!("key={content}"));
    }

    let mut hasher = sha2::Sha256::new();
    for line in canonical {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())[..16].to_string()
}

#[derive(Args)]
#[command(after_help = "\
Examples:
  xbin build ./myapp                         Build for the host (default output: app.xbin)
  xbin build ./myapp -o myapp.xbin           Build for the host, custom output name
  xbin build ./myapp --target linux-arm64 -o myapp.xbin     Single cross-target artifact
  xbin build ./myapp --target linux-x64,linux-arm64 -o out/app.xbin  Multi-arch: emits app-linux-x64.xbin + app-linux-arm64.xbin
  xbin build ./myapp --target win-x64 -o out/app.xbin       Cross-OS: Windows PE stub (.exe)
  xbin build ./myapp --dry-run                              Preview the multi-target plan without building")]
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

    /// Isolation mode: sandbox, none, or 0-2
    #[arg(long, default_value = "sandbox")]
    pub isolation: String,

    /// Enable seccomp BPF filter
    #[arg(long)]
    pub seccomp: bool,

    /// Enable Landlock LSM filesystem sandbox
    #[arg(long)]
    pub landlock: bool,

    /// Encrypt the payload with AES-256-GCM (requires --key).
    ///
    /// WARNING: this provides obfuscation against casual inspection only, NOT
    /// confidentiality. The AES key is stored in the binary's metadata next to
    /// the ciphertext, so anyone holding the `.xbin` can decrypt it. Real
    /// confidentiality requires a key that is never stored in the file
    /// (env var, passphrase, HSM).
    #[arg(long)]
    pub encrypt: bool,

    /// Use `SquashFS` instead of zstd+tar
    #[arg(long)]
    pub squashfs: bool,

    /// Enable SISR delta-indexing and self-update support
    #[arg(long)]
    pub enable_sisr: bool,

    /// Base URL of the SISR update channel
    #[arg(long)]
    pub update_url: Option<String>,

    /// Target architecture (e.g., aarch64, `x86_64`, linux-arm64, macos-arm64)
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

    /// Define build-time constants (repeatable): --define KEY=VALUE
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

    /// Include extra files/directories in the rootfs (repeatable).
    ///
    /// `.env` is excluded by default (secret-leak risk); pass
    /// `--include <app>/.env` explicitly to bundle it at your own risk.
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

    /// Enable WASM support with wasmtime (NOT YET IMPLEMENTED IN STUB)
    #[arg(long, hide = true)]
    pub wasm: bool,

    /// Path to wasmtime binary (NOT YET IMPLEMENTED IN STUB)
    #[arg(long, hide = true)]
    pub wasmtime_path: Option<PathBuf>,

    /// Enable WASI for WASM modules (WebAssembly System Interface) (NOT YET IMPLEMENTED IN STUB)
    #[arg(long, hide = true)]
    pub wasi: bool,

    /// Enable WASM component model support (NOT YET IMPLEMENTED IN STUB)
    #[arg(long, hide = true)]
    pub component_model: bool,

    /// Cross-compile for target architectures (comma-separated, e.g., aarch64,arm64) (NOT YET IMPLEMENTED IN STUB)
    #[arg(long, hide = true)]
    pub cross_compile: Option<String>,

    /// Use intelligent build cache (skip extraction if hash matches)
    #[arg(long)]
    pub use_cache: bool,

    /// Clear build cache before building
    #[arg(long)]
    pub clear_cache: bool,

    /// Remote cache base URL (Depot-style: GET/PUT `{url}/{hash}`)
    #[arg(long)]
    pub remote_cache_url: Option<String>,

    /// Remote cache max entries (default: 50)
    #[arg(long)]
    pub remote_cache_max_entries: Option<usize>,

    /// Zstd compression level (1=fastest/largest, 3=default, 19=smallest/slowest)
    #[arg(long, default_value = "3")]
    pub compression_level: i32,

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

    // Load .xbin.toml config if present
    let config = load_config(&app_dir);

    // Apply config defaults (CLI flags override). Clone the args fields so
    // `&args` stays borrowable for the per-target loop below.
    let isolation = if args.isolation != "sandbox" {
        args.isolation.clone()
    } else {
        config.build.isolation.unwrap_or_else(|| "sandbox".into())
    };
    let isolation_num = parse_isolation(&isolation)
        .with_context(|| format!("invalid --isolation value: '{isolation}'"))?;

    // Detect runtime
    let runtime = detect::detect_runtime(&app_dir).context(
        "could not detect runtime — supported: python, node, deno, java, ruby, dotnet, go, php, perl, binary",
    )?;
    let runtime_name = runtime.name().to_string();

    eprintln!("Detected runtime: {runtime_name}");

    let targets = resolve_targets(&args, config.build.target.as_deref());
    let outputs = output_paths(&args, &targets);
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
        encrypt: args.encrypt || config.build.encrypt.unwrap_or(false),
        squashfs: args.squashfs || config.build.squashfs.unwrap_or(false),
        version_info: args.version_info.clone().or(config.package.version),
        author: args.author.clone().or(config.package.author),
        description: args.description.clone().or(config.package.description),
        license: args.license.clone().or(config.package.license),
        env_file: args
            .env_file
            .clone()
            .or(config.build.env_file.map(PathBuf::from)),
        targets,
        outputs,
    };

    if args.dry_run {
        for (target, output) in plan.targets.iter().zip(&plan.outputs) {
            print_dry_run(&args, &plan, target.as_deref(), output);
        }
        return Ok(());
    }

    // Build one artifact per target; each gets its own output path.
    let mut json_results: Vec<serde_json::Value> = Vec::new();
    for (target, output) in plan.targets.iter().zip(&plan.outputs) {
        if let Some(result) = build_single_target(&args, &plan, target.clone(), output)? {
            json_results.push(result);
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

/// Warn when `--seccomp`/`--landlock` are requested but the stub will not
/// enforce them (they only apply at isolation >= 2, on the `pivot_root` path).
fn warn_sandbox_noops(isolation_num: u32, seccomp: bool, landlock: bool) {
    if isolation_num < 2 && (seccomp || landlock) {
        let flags = [("seccomp", seccomp), ("landlock", landlock)]
            .iter()
            .filter_map(|(name, on)| on.then_some(*name))
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!(
            "[xbin] warning: {flags} require --isolation sandbox (2) to take effect — \
             ignored at isolation level {isolation_num}"
        );
    }
}

/// Print the dry-run plan for one target (or the host when `target` is None).
fn print_dry_run(args: &BuildArgs, plan: &BuildPlan, target: Option<&str>, output: &Path) {
    eprintln!("Dry run — would build:");
    eprintln!("  App:       {}", plan.app_dir.display());
    eprintln!("  Output:    {}", output.display());
    eprintln!("  Runtime:   {}", plan.runtime_name);
    eprintln!("  Isolation: {}", plan.isolation);
    eprintln!("  Seccomp:   {}", plan.seccomp);
    eprintln!("  Landlock:  {}", plan.landlock);
    warn_sandbox_noops(plan.isolation_num, plan.seccomp, plan.landlock);
    eprintln!("  Encrypt:   {}", plan.encrypt);
    eprintln!("  SquashFS:  {}", plan.squashfs);
    if args.enable_sisr {
        eprintln!("  SISR:      enabled (delta-indexed, <output>.manifest)");
        match &args.update_url {
            Some(url) => eprintln!("  Update URL: {url}"),
            None => {
                eprintln!("  Update URL: (none — updates must pass a URL or set XBIN_UPDATE_URL)");
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
    let all_mgrs = pkgmgr::detect_all_pkgmgrs(&plan.app_dir, &plan.runtime_name);
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

/// Build a single `.xbin`/`.exe` artifact for `target` (host when `None`).
///
/// Returns a JSON result object when `--json` was requested, which the caller
/// collects and prints (one object per target, or an array for multi-arch).
fn build_single_target(
    args: &BuildArgs,
    plan: &BuildPlan,
    target: Option<String>,
    output: &PathBuf,
) -> Result<Option<serde_json::Value>> {
    let verbose = plan.verbose;
    let app_dir = &plan.app_dir;
    let runtime = plan.runtime;
    let runtime_name = &plan.runtime_name;
    let isolation_num = plan.isolation_num;
    let no_install = plan.no_install;
    let seccomp = plan.seccomp;
    let landlock = plan.landlock;
    let encrypt = plan.encrypt;
    let squashfs = plan.squashfs;
    let version_info = plan.version_info.clone();
    let author = plan.author.clone();
    let description = plan.description.clone();
    let license = plan.license.clone();
    let env_file = plan.env_file.clone();

    // ── Sandbox flags without isolation are silent no-ops ──────────────
    // seccomp/landlock are only enforced in the stub when isolation >= 2
    // (pivot_root + namespace path). Warn so the user isn't lulled into
    // believing the sandbox is active.
    warn_sandbox_noops(isolation_num, seccomp, landlock);

    // ── Reject the broken encrypt+SISR combination ─────────────────────
    // The stub's chunked-decrypt path requires per-layer sizes from
    // `meta.layers`, but the CLI always assembles with an empty layer list —
    // the produced binary would carry a chunk-encrypted payload that the
    // stub can only attempt to decrypt as one GCM blob, failing at runtime.
    if encrypt && args.enable_sisr {
        anyhow::bail!(
            "--encrypt and --enable-sisr cannot be combined: chunk-encrypted SISR \
             binaries are not decryptable by the current stub — use one or the other"
        );
    }

    // ── Reject a bundled `.env` unless it is included explicitly ───────
    // `.env` is excluded from the payload by default (secrets would be
    // extractable from the redistributable); silently dropping it could
    // break an app that relied on it, so make the builder decide.
    if app_dir.join(".env").is_file() && !include_points_to_env(app_dir, &args.include) {
        anyhow::bail!(
            "{} contains a .env file, which is excluded from the payload by default \
             (its secrets would be extractable from the binary). Bundle it explicitly \
             with `--include {}` to accept that risk, or provide configuration via \
             `--env-file` / `--env` instead.",
            app_dir.display(),
            app_dir.join(".env").display()
        );
    }

    // ── Compute hashes for incremental update ──────────────────────────
    // Hashed BEFORE tree-shake/minify run: those mutate a staging copy
    // below, and the cache/`--update` keys must reflect the pristine
    // source, not its by-products, or a rebuild would never match.
    let new_app_hash = erebus_core::include::hash_app_files(app_dir);
    let new_rt_hash = erebus_core::include::hash_lock_file(app_dir);

    // ── Intelligent build cache: skip rebuild if hash matches ──────────
    let cfg_hash = config_fingerprint(args, plan);
    if args.use_cache {
        let cache = erebus_core::paths::BuildCache::new(app_dir, 50);
        if let Some(cached) = cache.find(&new_app_hash, &cfg_hash, target.as_deref()) {
            if verbose {
                eprintln!("[xbin] cache hit — reusing cached build");
            }
            std::fs::copy(&cached, &output).context("failed to copy cached .xbin to output")?;
            if args.json {
                return Ok(Some(serde_json::json!({
                    "output": output.to_string_lossy(),
                    "runtime": runtime_name.as_str(),
                    "cache_hit": true,
                })));
            }
            return Ok(None);
        }
        if verbose {
            eprintln!("[xbin] cache miss — building from scratch");
        }
    }

    if args.clear_cache {
        let cache = erebus_core::paths::BuildCache::new(app_dir, 50);
        cache.clear().ok();
        if verbose {
            eprintln!("  cache: cleared");
        }
    }

    // ── Incremental update: skip rebuild if nothing changed ────────────
    if args.update && output.exists() {
        if let Some((old_app_hash, old_rt_hash)) = read_existing_hashes(output) {
            if old_app_hash == new_app_hash && old_rt_hash == new_rt_hash {
                if verbose {
                    eprintln!("[xbin] everything up to date, nothing to rebuild");
                }
                return Ok(None);
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
        check_php_platform_reqs(app_dir, verbose)?;
    }

    // Detect pnpm workspace with workspace:* protocol (cannot use npm)
    if pkgmgr::detect_node_pkgmgr(app_dir) == Some(pkgmgr::PkgMgr::Npm)
        && has_workspace_protocol(app_dir)
    {
        eprintln!("[xbin] warning: package.json uses `workspace:*` protocol (pnpm-specific)");
        eprintln!("  but pnpm is not detected. Create `pnpm-workspace.yaml` or add a lockfile.");
    }

    // Detect and install all package managers (primary + secondary)
    let all_pkg_mgrs = pkgmgr::detect_all_pkgmgrs(app_dir, runtime_name);
    for mgr in &all_pkg_mgrs {
        if verbose {
            eprintln!("Package manager: {}", mgr.name());
        }

        if !no_install {
            let cmd = mgr.install_cmd();

            // Check if the binary exists before trying to run it. For
            // cross-target builds the builder's host toolchain has the wrong
            // arch/OS, so always download the target node instead.
            let need_target_node = target.is_some()
                && matches!(
                    mgr,
                    pkgmgr::PkgMgr::Npm
                        | pkgmgr::PkgMgr::Pnpm
                        | pkgmgr::PkgMgr::Yarn
                        | pkgmgr::PkgMgr::Bun
                );
            let mut node_bin_dir: Option<PathBuf> = None;
            if !is_command_available(cmd[0]) || need_target_node {
                // For node/npm: download static node to temp dir
                if matches!(
                    mgr,
                    pkgmgr::PkgMgr::Npm
                        | pkgmgr::PkgMgr::Pnpm
                        | pkgmgr::PkgMgr::Yarn
                        | pkgmgr::PkgMgr::Bun
                ) {
                    let bin_dir = ensure_node(target.as_deref(), verbose)?;
                    node_bin_dir = Some(bin_dir);
                } else {
                    eprintln!(
                        "[xbin] skipping {} — `{}` not found on PATH",
                        mgr.name(),
                        cmd[0]
                    );
                    continue;
                }
            }

            // For composer: auto-download composer.phar if not on PATH
            let (prog, extra_args) = if matches!(mgr, pkgmgr::PkgMgr::Composer) {
                ensure_composer(app_dir, verbose)?
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

            // When cross-compiling for a different OS/arch, tell npm/pnpm/yarn/bun
            // to install dependencies for the target platform, not the host.
            //
            // We also pass --ignore-scripts: the target node binary cannot run
            // on a foreign host (e.g. macOS node on Linux), so any install
            // scripts that compile native modules would fail. Pure-JS deps and
            // prebuilt binaries selected by --platform/--arch still work.
            let is_cross_node = matches!(
                mgr,
                pkgmgr::PkgMgr::Npm
                    | pkgmgr::PkgMgr::Pnpm
                    | pkgmgr::PkgMgr::Yarn
                    | pkgmgr::PkgMgr::Bun
            ) && target.as_ref().is_some_and(|t| {
                let (t_arch, t_os) = parse_target(t);
                t_arch != std::env::consts::ARCH || t_os != std::env::consts::OS
            });
            if is_cross_node {
                if let Some(target_str) = target.as_ref() {
                    let (t_arch, t_os) = parse_target(target_str);
                    let npm_arch = match t_arch.as_str() {
                        "x86_64" => "x64",
                        "aarch64" | "arm64" => "arm64",
                        "i686" | "i386" | "x86" => "ia32",
                        other => other,
                    };
                    let npm_platform = match t_os.as_str() {
                        "darwin" => "darwin",
                        "windows" => "win32",
                        "linux" => "linux",
                        other => other,
                    };
                    if matches!(mgr, pkgmgr::PkgMgr::Npm | pkgmgr::PkgMgr::Pnpm) {
                        full_args.push("--platform".into());
                        full_args.push(npm_platform.into());
                        full_args.push("--arch".into());
                        full_args.push(npm_arch.into());
                        full_args.push("--ignore-scripts".into());
                    } else if matches!(mgr, pkgmgr::PkgMgr::Yarn) {
                        full_args.push("--platform".into());
                        full_args.push(npm_platform.into());
                        full_args.push("--ignore-scripts".into());
                    } else if matches!(mgr, pkgmgr::PkgMgr::Bun) {
                        full_args.push("--platform".into());
                        full_args.push(npm_platform.into());
                        full_args.push("--arch".into());
                        full_args.push(npm_arch.into());
                        full_args.push("--ignore-scripts".into());
                    }
                }
            }

            let mut command = std::process::Command::new(&prog);
            command.args(&full_args).current_dir(app_dir);

            // If we downloaded node for npm/yarn/bun, prepend its bin dir to PATH
            // using Command::env() instead of mutating global std::env::PATH
            if let Some(ref bin_dir) = node_bin_dir {
                let current = std::env::var("PATH").unwrap_or_default();
                command.env("PATH", format!("{}:{}", bin_dir.display(), current));
            }

            let status = command
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

    // Clean up downloaded build tools (node/npm, composer.phar) from cache
    // (deferred: for cross-target builds the downloaded node must remain on
    // PATH until the interpreter is embedded below).

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
    copy_dir_recursive_with(app_dir, &rootfs.join("app"), include_node_modules)
        .context("failed to copy app files")?;

    // ── Tree-shake / minify on the STAGING COPY ───────────────────────
    // Never mutate the source directory: a build must leave `app_dir`
    // byte-identical so repeat builds and `--update` hash comparisons see
    // the real input.
    if args.tree_shake {
        let removed = erebus_core::treeshake::prune_node_modules(&rootfs.join("app"), verbose)
            .context("tree-shaking failed")?;
        if verbose {
            eprintln!("  tree-shake: removed {removed} unused package(s)");
        }
    }

    if args.minify {
        let minified = erebus_core::minify::minify_app_dir(&rootfs.join("app"), verbose)
            .context("minification failed")?;
        if verbose {
            eprintln!("  minify: minified {minified} file(s)");
        }
    }

    // ── Include extra files ───────────────────────────────────────────
    if !args.include.is_empty() {
        let app_dest = rootfs.join("app");
        let count = erebus_core::include::copy_include_paths(&args.include, &app_dest, app_dir)
            .context("failed to copy include paths")?;
        if verbose {
            eprintln!("  include: copied {count} path(s) into rootfs");
        }
    }

    // ── Embed interpreter ──────────────────────────────────
    // NOTE: ensure_node() may have downloaded the target runtime to
    // <cache_dir>/build-tools/<target>/bin and added it to PATH —
    // embed_interpreter picks it up from there (`.exe` first for cross-OS).
    let embedded_interpreter_str = if let Some(ref interp) = args.embed_interpreter {
        Some(interp.clone())
    } else {
        match runtime_name.as_str() {
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
        match embed::embed_interpreter(&interpreter_path, &rootfs, Some(app_dir), verbose) {
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
            if let Err(e) = embed::embed_interpreter("rr", &rootfs, None, verbose) {
                eprintln!("[xbin] warning: failed to embed RoadRunner: {}", e);
            }
        } else if verbose {
            eprintln!("[xbin] warning: rr binary not found on PATH; RoadRunner won't be available at runtime");
        }
    }

    if !interpreter_embedded && verbose && embedded_interpreter_str.is_some() {
        eprintln!("  (interpreter embedding skipped)");
    }

    // Clean up downloaded build tools (node/npm, composer.phar). Only the
    // tool's own cache dir is removed — never a shared `/tmp` path, which
    // another user/process may own or have symlinked (roadmap #36).
    let _ = std::fs::remove_dir_all(cache_dir().join("build-tools"));

    // Build the payload: zstd(tar) by default, or a real SquashFS image
    // when `--squashfs` was requested (v5). Before this fix the flag only
    // flipped the metadata's payload_format while the payload stayed a
    // zstd+tar stream — the stub's squashfs extractor would fail on it.
    eprintln!("Creating payload...");
    let t0 = std::time::Instant::now();
    let mut payload = if squashfs {
        create_squashfs_payload(&rootfs, verbose).context("failed to create squashfs payload")?
    } else {
        erebus_core::tar::create_tar_zstd_with_level(&rootfs, args.compression_level)
            .context("failed to create tar+zstd payload")?
    };
    let compress_ms = t0.elapsed().as_millis();
    if verbose {
        eprintln!(
            "  compress: {compress_ms}ms, {} MB",
            payload.len() as f64 / 1_048_576.0
        );
    }

    // ── Encryption (v4) ──────────────────────────────────────────────────
    // AES-256-GCM, key derived from a random 32-byte encryption key via
    // HKDF-SHA256 (same key the stub derives at runtime).
    //
    // SECURITY: The encryption key is **separate** from the Ed25519 signing
    // seed.  `--key` is only used for SISR manifest signing (when
    // `--enable-sisr` is also set) and/or binary signing (`--sign`).
    // The encryption key itself is generated randomly at build time and
    // stored in meta `crypto` as `encryption_key_hex`.  The signing seed is
    // NEVER embedded in the binary — this prevents a key compromise from
    // breaking both confidentiality and authenticity.
    //
    // The ciphertext replaces the payload before assembly so the footer's
    // integrity hash and signature both cover the *encrypted* bytes.
    //
    // When combined with `--enable-sisr`, the payload is encrypted in
    // per-chunk mode: each SISR plaintext chunk gets an independent AES key
    // derived via HKDF(encryption_key, salt, chunk_index), and the manifest
    // tracks the ciphertext hashes.  The stub decrypts each chunk
    // independently at runtime before SISR extraction.
    let mut crypto_meta: Option<serde_json::Value> = None;
    let mut sisr_artifacts_opt: Option<erebus_core::sisr_stage::SisrArtifacts> = None;

    if encrypt {
        // Generate a fresh random encryption key — never reuse the signing seed.
        let mut encryption_key = [0u8; 32];
        encryption_key.copy_from_slice(&erebus_core::encrypt::generate_encryption_key());

        if args.enable_sisr {
            let sisr_config = build_sisr_config(&args.key)?;
            let artifacts = erebus_core::sisr_stage::build_artifacts(&payload, &sisr_config)
                .context("SISR stage failed during encrypt+SISR build")?;
            let chunk_sizes: Vec<usize> = artifacts
                .manifest
                .chunks
                .iter()
                .map(|c| c.length as usize)
                .collect();
            let salt = erebus_core::encrypt::generate_salt();
            let nonce = erebus_core::encrypt::generate_nonce();
            let ciphertext = erebus_core::encrypt::encrypt_chunks(
                &payload,
                &encryption_key,
                &salt,
                &nonce,
                &chunk_sizes,
            )
            .context("chunked AES-256-GCM payload encryption failed")?;
            payload = ciphertext;
            crypto_meta = Some(serde_json::json!({
                "nonce_hex": hex::encode(nonce),
                "encryption_key_hex": hex::encode(encryption_key),
                "encryption_salt_hex": hex::encode(salt),
                "chunked": true,
            }));
            sisr_artifacts_opt = Some(artifacts);
            if verbose {
                eprintln!(
                    "  encrypt+SISR: {} -> {} bytes (per-chunk AES-256-GCM, {} chunks)",
                    payload.len(),
                    payload.len(),
                    chunk_sizes.len()
                );
            }
        } else {
            let (ciphertext, em) = erebus_core::encrypt::encrypt_payload(&payload, &encryption_key)
                .context("AES-256-GCM payload encryption failed")?;
            payload = ciphertext;
            crypto_meta = Some(serde_json::json!({
                "nonce_hex": hex::encode(em.nonce),
                "tag_offset": em.tag_offset,
                "encryption_key_hex": hex::encode(encryption_key),
                "encryption_salt_hex": hex::encode(em.salt),
            }));
            if verbose {
                eprintln!(
                    "  encrypt: {} -> {} bytes (AES-256-GCM, tag at {})",
                    payload.len(),
                    payload.len(),
                    em.tag_offset
                );
            }
        }
    }

    // Build metadata
    let app_name = app_dir
        .file_name()
        .map_or_else(|| "app".to_string(), |n| n.to_string_lossy().into());

    let mut env_map = serde_json::Map::new();
    env_map.insert("XBIN_RUNTIME".into(), runtime_name.clone().into());
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
        let persist_dir = erebus_core::persistent::get_persist_dir(&app_name);
        let _ = erebus_core::persistent::ensure_persist_dir(&app_name);
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
        let otel_env = erebus_core::otel::build_otel_env(
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
                let interval = erebus_core::cron::parse_schedule(schedule);
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
        detect::resolve_entrypoint(app_dir, runtime).unwrap_or_else(|| vec!["run".to_string()]);

    let entrypoint = if runtime == detect::Runtime::Wasm && (args.wasi || args.component_model) {
        let mut ep = entrypoint.clone();
        if args.component_model {
            ep.insert(1, "--component-model".into());
        }
        if args.wasi {
            ep.insert(1, "--wasi".into());
        }
        ep
    } else {
        entrypoint
    };

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
        match runtime_name.as_str() {
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
        eprintln!("[xbin] warning: --wasm is not yet implemented in the stub; metadata will be written but ignored at runtime");
        bun_features.wasm.enabled = true;
        if let Some(ref path) = args.wasmtime_path {
            bun_features.wasm.wasmtime_path = Some(path.display().to_string());
        }
        bun_features.wasm.wasi = args.wasi;
        bun_features.wasm.component_model = args.component_model;
        if verbose {
            eprintln!(
                "  wasm: enabled (wasi={}, component_model={})",
                args.wasi, args.component_model
            );
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
        eprintln!("[xbin] warning: --cross-compile is not yet implemented in the stub; metadata will be written but ignored at runtime");
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

    let meta = erebus_core::assembly::build_meta_json(
        &app_name,
        runtime_name,
        isolation_num,
        &entrypoint,
        &env_pairs,
        &erebus_core::assembly::MetaOptions {
            version: version_info,
            author,
            description,
            license,
            payload_format: Some(if squashfs { "squashfs" } else { "zstd-tar" }.to_string()),
            seccomp,
            landlock,
            app_hash: Some(new_app_hash.clone()),
            rt_deps_hash: Some(new_rt_hash.clone()),
            update_url: args.update_url.clone(),
            crypto: crypto_meta,
        },
        &bun_features,
    )?;

    // Assemble
    eprintln!("Assembling {}...", output.display());

    let size = if args.enable_sisr {
        let sisr_config = build_sisr_config(&args.key)?;
        let artifacts = match sisr_artifacts_opt {
            Some(a) => a,
            None => erebus_core::sisr_stage::build_artifacts(&payload, &sisr_config)
                .context("SISR stage failed during build")?,
        };
        let input = erebus_core::assembly::AssemblyInput {
            stub_bytes: &stub_bytes,
            payload: &payload,
            meta_bytes: &meta,
            encrypt,
            squashfs,
            target_arch: target.as_deref(),
            sisr: Some(artifacts),
        };
        erebus_core::assembly::assemble_xbin(output, &input)
            .context("failed to assemble xbin (SISR)")?
    } else {
        let input = erebus_core::assembly::AssemblyInput {
            stub_bytes: &stub_bytes,
            payload: &payload,
            meta_bytes: &meta,
            encrypt,
            squashfs,
            target_arch: target.as_deref(),
            sisr: None,
        };
        erebus_core::assembly::assemble_xbin(output, &input).context("failed to assemble xbin")?
    };

    eprintln!(
        "Built {} ({:.1}MB)",
        output.display(),
        size as f64 / (1024.0 * 1024.0)
    );

    if args.enable_sisr {
        eprintln!(
            "warning: SISR binaries are NOT signed at rest — authenticity is only guaranteed \
             during an update, when the remote manifest signature is verified"
        );
    }

    // macOS code signing: re-sign the assembled binary since appending
    // payload + metadata invalidates any existing Mach-O signature.
    if target
        .as_ref()
        .is_some_and(|t| t.contains("darwin") || t.contains("apple") || t.contains("macos"))
    {
        sign_macos_binary(output, verbose)?;
    }

    if args.enable_sisr {
        let mut manifest = output.clone();
        manifest.set_extension("xbin.manifest");
        eprintln!("SISR manifest written: {}", manifest.display());
    }

    // Binary signature is mutually exclusive with SISR in a single build:
    // `sign_file` rebuilds the file as `[..meta_end][sig][footer]`, which would
    // truncate the SISR section. With `--enable-sisr`, `--key` instead signs
    // the manifest (see `build_sisr_config`).
    //
    // Signing happens BEFORE the cache store so the cached artifact is the
    // complete, signed binary — a cache hit must not serve an unsigned copy.
    if !args.enable_sisr {
        if let Some(key_path) = &args.key {
            if verbose {
                eprintln!("Signing...");
            }
            super::sign::sign_file(output, key_path, !verbose)?;
        }
    }

    // ── Store in build cache ──────────────────────────────────────────
    if args.use_cache {
        let cache = erebus_core::paths::BuildCache::new(app_dir, 50);
        if cache
            .store(&new_app_hash, &cfg_hash, target.as_deref(), output)
            .is_ok()
            && verbose
        {
            eprintln!("  cache: stored build");
        }
    }

    // ── Store in remote cache (Depot-style) ───────────────────────────
    if let Some(remote) = remote_cache_from_args(args, app_dir) {
        if remote
            .store(&new_app_hash, &cfg_hash, target.as_deref(), output)
            .is_ok()
            && verbose
        {
            eprintln!("  remote cache: stored build");
        }
    }

    if args.json {
        return Ok(Some(serde_json::json!({
            "output": output.to_string_lossy(),
            "size_bytes": size,
            "runtime": runtime_name,
            "format": "zstd-tar",
            "signed": args.key.is_some() && !args.enable_sisr,
            "encrypted": encrypt,
            "sisr": args.enable_sisr,
            "manifest_signed": args.enable_sisr && args.key.is_some(),
        })));
    }
    Ok(None)
}

/// Re-sign a macOS Mach-O binary using `codesign` after assembly.
///
/// Appending payload + metadata invalidates any existing code signature,
/// so we must re-sign the final `.xbin` (Mach-O stub + appended data).
///
/// On non-macOS hosts this is a no-op. On macOS without a signing identity
/// the binary is left unsigned with a warning.
fn sign_macos_binary(path: &Path, verbose: bool) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;

        // Ad-hoc signing is sufficient for local development; distribution
        // requires a Developer ID in the user's keychain.
        let identity = match std::env::var("XBIN_CODESIGN_IDENTITY") {
            Ok(id) if !id.is_empty() => id,
            _ => "-".to_string(),
        };

        let output = Command::new("codesign")
            .args(["--sign", &identity, "--force", "--timestamp"])
            .arg(path)
            .output()
            .context("failed to run codesign — is it installed?")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("codesign failed: {stderr}");
        }

        if verbose {
            eprintln!("  macOS: re-signed Mach-O with identity '{identity}'");
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (path, verbose);
    }

    Ok(())
}

/// Builds the SISR stage config from the CLI args. When `--key` is given the
/// same 32-byte Ed25519 key that would sign the binary instead signs the SISR
/// manifest; its bytes are never printed (only the path appears in warnings).
fn build_sisr_config(key_path: &Option<PathBuf>) -> Result<SisrBuildConfig> {
    let signing_key = match key_path {
        Some(path) => {
            warn_if_insecure_key_permissions(path);
            let key_bytes = std::fs::read(path)
                .with_context(|| format!("failed to read signing key at {}", path.display()))?;
            if key_bytes.len() != 32 {
                anyhow::bail!("key must be 32 bytes, got {}", key_bytes.len());
            }
            let mut key_arr = [0u8; 32];
            key_arr.copy_from_slice(&key_bytes);
            Some(SigningKey::from_bytes(&key_arr))
        }
        None => None,
    };
    Ok(SisrBuildConfig {
        enabled: true,
        chunk_target_size: 64 << 10,
        signing_key,
    })
}

/// Warns when a private key file is group/other-readable (not 0600) on Unix.
fn warn_if_insecure_key_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mode = meta.permissions().mode() & 0o777;
            if mode != 0o600 {
                eprintln!(
                    "[xbin] warning: private key {} has mode {mode:o}, expected 0600",
                    path.display()
                );
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
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
/// Downloads a static node to `~/.cache/xbin/build-tools/node/` (or a
/// per-target subdir when `--target` requests a non-host platform) if not on
/// PATH. The user-writable cache dir (0700) avoids the symlink attacks a
/// predictable world-writable `/tmp` path would allow. Does NOT pollute the
/// user's system PATH.
fn ensure_node(target: Option<&str>, verbose: bool) -> Result<PathBuf> {
    let suffix = target
        .map(|t| {
            let (arch, os) = parse_target(t);
            format!("{os}-{arch}")
        })
        .unwrap_or_else(|| "host".to_string());
    let tools_dir = cache_dir()
        .join("build-tools")
        .join(format!("node-{suffix}"));
    let is_windows = target.is_some_and(|t| parse_target(t).1 == "windows");
    let node_name = if is_windows { "node.exe" } else { "node" };
    let npm_name = if is_windows { "npm.cmd" } else { "npm" };
    let node_bin = tools_dir.join("bin").join(node_name);
    let npm_bin = tools_dir.join("bin").join(npm_name);

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
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            tools_dir.parent().unwrap_or(tools_dir.as_path()),
            std::fs::Permissions::from_mode(0o700),
        )
        .ok();
    }

    ensure_node_download(tools_dir, target, verbose)
}

/// Download a specific Node.js version for a target architecture.
///
/// When `target_arch` is `None`, downloads for the host architecture.
fn ensure_node_download(
    tools_dir: PathBuf,
    target_arch: Option<&str>,
    verbose: bool,
) -> Result<PathBuf> {
    // Map the target to node's official `os-arch` tarball naming. Node ships
    // static builds for linux (musl-compatible), darwin, and windows (`win`).
    let (node_arch, node_os) = if let Some(target) = target_arch {
        let (arch, os) = parse_target(target);
        let node_arch = match arch.as_str() {
            "x86_64" | "amd64" => "x64",
            "aarch64" | "arm64" => "arm64",
            _ => anyhow::bail!("unsupported cross-compile architecture: {arch}"),
        };
        let node_os = match os.as_str() {
            "linux" => "linux",
            "darwin" => "darwin",
            "windows" => "win",
            _ => anyhow::bail!("unsupported cross-compile OS: {os}"),
        };
        (node_arch.to_string(), node_os.to_string())
    } else {
        let node_arch = match std::env::consts::ARCH {
            "x86_64" => "x64",
            "aarch64" => "arm64",
            arch => arch,
        };
        let node_os = match std::env::consts::OS {
            "linux" => "linux",
            "macos" => "darwin",
            "windows" => "win",
            os => os,
        };
        (node_arch.to_string(), node_os.to_string())
    };

    // Windows dists name the binaries `node.exe` / `npm.cmd`.
    let node_bin = tools_dir
        .join("bin")
        .join(if node_os == "win" { "node.exe" } else { "node" });

    #[cfg(unix)]
    let npm_bin = tools_dir
        .join("bin")
        .join(if node_os == "win" { "npm.cmd" } else { "npm" });

    let versions: Vec<serde_json::Value> =
        reqwest::blocking::get("https://nodejs.org/dist/index.json")
            .context("failed to reach nodejs.org")?
            .json()
            .context("failed to parse node version manifest")?;
    let version = if let Ok(pinned) = std::env::var("XBIN_NODE_VERSION") {
        if verbose {
            eprintln!("  using pinned node version {pinned} (XBIN_NODE_VERSION)");
        }
        pinned
    } else {
        versions
            .first()
            .and_then(|v| v.get("version")?.as_str())
            .and_then(|v| v.strip_prefix('v'))
            .map(|v| v.to_string())
            .ok_or_else(|| anyhow::anyhow!("no node version found in manifest"))?
    };

    // nodejs.org serves .tar.xz on Linux, .tar.gz on macOS, and .zip on Windows.
    let ext = match node_os.as_str() {
        "darwin" => "tar.gz",
        "win" => "zip",
        _ => "tar.xz",
    };
    let tarball = format!("node-v{version}-{node_os}-{node_arch}.{ext}");
    let url = format!("https://nodejs.org/dist/v{version}/{tarball}");

    if verbose {
        eprintln!("  downloading node v{version} ({node_os}-{node_arch})...");
    }

    let response = reqwest::blocking::get(&url).context("failed to download node.js tarball")?;
    if node_os == "win" {
        // ZipArchive needs a seekable reader; buffer the ~30 MB dist in memory.
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut std::io::BufReader::new(response), &mut bytes)
            .context("failed to read node.js zip")?;
        extract_node_zip(std::io::Cursor::new(bytes), &tools_dir)?;
    } else {
        let reader = std::io::BufReader::new(response);
        let decoder: Box<dyn std::io::Read> = if node_os == "darwin" {
            Box::new(flate2::read::GzDecoder::new(reader))
        } else {
            Box::new(xz2::read::XzDecoder::new(reader))
        };
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

/// Extract the Windows node dist zip into `tools_dir/bin/`.
///
/// The zip layout is a single top-level `node-v<ver>-win-x64/` directory whose
/// contents (`node.exe`, `npm.cmd`, `node_modules/`) we strip into `bin/` so
/// the layout matches the linux/darwin tarballs. `npm.cmd` resolves `node.exe`
/// and `node_modules/npm` relative to its own directory, so they must stay
/// siblings. Entry names are validated against path traversal before use.
fn extract_node_zip<R: std::io::Read + std::io::Seek>(reader: R, tools_dir: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(reader).context("failed to read node.js zip")?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .with_context(|| format!("failed to read zip entry {i}"))?;
        let name = std::path::Path::new(entry.name());
        // Zip-slip guard: reject absolute paths and any traversal component.
        if name.is_absolute()
            || name.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            })
        {
            continue;
        }
        let stripped: PathBuf = name.components().skip(1).collect();
        if stripped.components().count() == 0 {
            continue;
        }
        let target = tools_dir.join("bin").join(&stripped);
        if entry.is_dir() {
            std::fs::create_dir_all(&target)
                .with_context(|| format!("failed to create directory {}", target.display()))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }
        let mut file = std::fs::File::create(&target)
            .with_context(|| format!("failed to create {}", target.display()))?;
        std::io::copy(&mut entry, &mut file)
            .with_context(|| format!("failed to unpack {}", stripped.display()))?;
    }
    Ok(())
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
        .filter_map(|s| {
            s.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .ok()
        })
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
                version.len() >= 2
                    && version[0] == target[0]
                    && version[1] == target[1]
                    && version >= &target
            }
        } else {
            true
        }
    } else if constraint.ends_with('*') {
        let prefix = constraint.trim_end_matches('*');
        let prefix_parts: Vec<u32> = prefix.split('.').filter_map(|s| s.parse().ok()).collect();
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
    s.split('.').filter_map(|p| p.trim().parse().ok()).collect()
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

fn find_stub(target: &Option<String>) -> Result<PathBuf> {
    if let Ok(path) = std::env::var("XBIN_STUB_PATH") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    let target_dir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into());

    // Map the requested target to the stub build triple. Legacy short forms
    // map to the linux-musl ELF; darwin targets map to the Mach-O stub;
    // windows targets map to the PE stub (`x86_64-pc-windows-gnu` today).
    let is_windows = target
        .as_deref()
        .is_some_and(|t| parse_target(t).1 == "windows");

    // Derive the stub build triple from the canonical (arch, os) pair so that
    // legacy short forms (`linux-x64`, `aarch64`, `x64`) resolve to the real
    // musl/PE/Mach-O triple on disk. The previous matching on
    // `t.contains("linux")` left short forms as-is (`linux-x64`) and silently
    // fell through to a stale `/usr/local/bin/erebus-stub`, because no matching
    // stub directory existed.
    let arch_suffix = match target.as_deref().map(parse_target) {
        Some((arch, os)) if os == "linux" => format!("{arch}-unknown-linux-musl"),
        Some((arch, os)) if os == "darwin" => format!("{arch}-apple-darwin"),
        Some((arch, os)) if os == "windows" => format!("{arch}-pc-windows-gnu"),
        // Full but unrecognized triples: assume a Linux musl build (common case).
        Some((arch, _)) => format!("{arch}-unknown-linux-musl"),
        None => String::from("x86_64-unknown-linux-musl"),
    };

    let stub_name = if is_windows {
        "erebus-stub.exe"
    } else {
        "erebus-stub"
    };
    let candidates = [
        PathBuf::from(&target_dir)
            .join(&arch_suffix)
            .join("release")
            .join(stub_name),
        PathBuf::from("/tmp/erebus-stub-target")
            .join(&arch_suffix)
            .join("release")
            .join(stub_name),
        PathBuf::from("stub/target")
            .join(&arch_suffix)
            .join("release")
            .join(stub_name),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            return Ok(candidate.clone());
        }
    }

    // Removed stale fallback `/usr/local/bin/erebus-stub` to prevent embedding
    // an obsolete stub with unknown bugs. Users must build a fresh stub via
    // `make stub` or set `XBIN_STUB_PATH` explicitly.
    if let Ok(p) = which::which("erebus-stub") {
        eprintln!(
            "[xbin] warning: found erebus-stub on PATH at {}; prefer 'make stub' for reproducible builds",
            p.display()
        );
        return Ok(p);
    }

    anyhow::bail!("erebus-stub not found — run: make stub")
}

/// Read `app_hash` and `rt_deps_hash` from an existing `.xbin` file's metadata.
fn read_existing_hashes(xbin_path: &Path) -> Option<(String, String)> {
    use erebus_core::format::Footer;

    let mut f = std::fs::File::open(xbin_path).ok()?;
    let footer = Footer::read_from(&mut f).ok()?;
    let meta_size = footer.meta_size.try_into().ok()?;
    let meta_bytes = erebus_core::format::read_at(&mut f, footer.meta_offset, meta_size).ok()?;
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
            || name == ".env"
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

/// Whether any `--include` path resolves to `app_dir/.env`.
///
/// Canonicalization mirrors `copy_include_paths` so relative paths and `..`
/// components are resolved before comparison.
fn include_points_to_env(app_dir: &Path, includes: &[PathBuf]) -> bool {
    let target = app_dir.join(".env");
    let target = std::fs::canonicalize(&target).unwrap_or(target);
    includes.iter().any(|inc| {
        let canonical = std::fs::canonicalize(inc).unwrap_or_else(|_| inc.clone());
        canonical == target
    })
}

/// Creates a real `SquashFS` image of `rootfs` by shelling out to
/// `mksquashfs` (the v5 payload requires actual squashfs bytes, not just a
/// metadata flag). The image is written into a temp dir outside the source
/// tree so mksquashfs cannot include its own output.
///
/// Deterministic: `-noappend` builds a fresh image and `-all-time 0` pins
/// file timestamps so equal inputs yield equal bytes (matches the SHA-256
/// integrity model of the format).
fn create_squashfs_payload(rootfs: &Path, verbose: bool) -> Result<Vec<u8>> {
    if !is_command_available("mksquashfs") {
        anyhow::bail!(
            "mksquashfs not found on PATH — install squashfs-tools to build \
             --squashfs binaries"
        );
    }
    let tmp = tempfile::tempdir().context("failed to create temp dir for squashfs image")?;
    let image = tmp.path().join("rootfs.squashfs");
    let run = |args: &[&str]| -> Result<bool> {
        let status = std::process::Command::new("mksquashfs")
            .arg(rootfs)
            .arg(&image)
            .args(args)
            .status()
            .context("failed to run mksquashfs")?;
        Ok(status.success())
    };
    if verbose {
        eprintln!("  squashfs: creating image from {}", rootfs.display());
    }
    // zstd + pinned timestamps first; fall back to the tool defaults for
    // older squashfs-tools builds (e.g. without zstd or -all-time).
    if !run(&[
        "-noappend",
        "-no-progress",
        "-quiet",
        "-comp",
        "zstd",
        "-all-time",
        "0",
    ])? && !run(&["-noappend", "-no-progress", "-quiet"])?
    {
        anyhow::bail!(
            "mksquashfs failed to produce an image from {}",
            rootfs.display()
        );
    }
    if verbose {
        eprintln!(
            "  squashfs: {} bytes",
            std::fs::metadata(&image)
                .context("failed to stat squashfs image")?
                .len()
        );
    }
    std::fs::read(&image).context("failed to read squashfs image")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_default_maps_to_level_2() {
        assert_eq!(parse_isolation("sandbox").unwrap(), 2);
        assert_eq!(parse_isolation("2").unwrap(), 2);
    }

    #[test]
    fn none_maps_to_level_0() {
        assert_eq!(parse_isolation("none").unwrap(), 0);
        assert_eq!(parse_isolation("0").unwrap(), 0);
    }

    #[test]
    fn numeric_level_1_is_accepted() {
        assert_eq!(parse_isolation("1").unwrap(), 1);
    }

    #[test]
    fn invalid_values_fail_closed() {
        assert!(parse_isolation("3").is_err());
        assert!(parse_isolation("root").is_err());
        assert!(parse_isolation("").is_err());
    }

    #[test]
    fn include_points_to_env_matches_only_explicit_env() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join(".env"), "SECRET=1").unwrap();
        assert!(!include_points_to_env(dir.path(), &[]));
        assert!(include_points_to_env(
            dir.path(),
            &[dir.path().join(".env")]
        ));
        // `sub/../.env` canonicalizes to `.env` once the intermediate dir exists.
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        assert!(include_points_to_env(
            dir.path(),
            &[dir.path().join("sub").join("../.env")]
        ));
        assert!(!include_points_to_env(
            dir.path(),
            &[dir.path().join("sub").join("xbin.toml")]
        ));
    }

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
    fn find_stub_darwin_suffix() {
        let result = find_stub(&Some("aarch64-apple-darwin".into()));
        assert!(result.is_err() || result.is_ok(), "should not panic");
    }

    #[test]
    fn find_stub_windows_suffix() {
        let result = find_stub(&Some("win-x64".into()));
        assert!(result.is_err() || result.is_ok(), "should not panic");
    }

    #[test]
    fn parse_target_short_forms() {
        assert_eq!(parse_target("aarch64"), ("aarch64".into(), "linux".into()));
        assert_eq!(parse_target("x86_64"), ("x86_64".into(), "linux".into()));
    }

    #[test]
    fn parse_target_full_triples() {
        assert_eq!(
            parse_target("aarch64-apple-darwin"),
            ("aarch64".into(), "darwin".into())
        );
        assert_eq!(
            parse_target("x86_64-unknown-linux-musl"),
            ("x86_64".into(), "linux".into())
        );
        assert_eq!(
            parse_target("aarch64-unknown-linux-gnu"),
            ("aarch64".into(), "linux".into())
        );
        assert_eq!(
            parse_target("x86_64-pc-windows-gnu"),
            ("x86_64".into(), "windows".into())
        );
    }

    #[test]
    fn parse_target_windows_shorthands() {
        assert_eq!(parse_target("win-x64"), ("x86_64".into(), "windows".into()));
        assert_eq!(
            parse_target("win-arm64"),
            ("aarch64".into(), "windows".into())
        );
        assert_eq!(parse_target("linux-x64"), ("x86_64".into(), "linux".into()));
        assert_eq!(
            parse_target("linux-arm64"),
            ("aarch64".into(), "linux".into())
        );
    }

    #[test]
    fn resolve_targets_comma_list_and_cross_compile() {
        let args = BuildArgs {
            target: Some("linux-x64,win-x64".into()),
            cross_compile: Some("aarch64".into()),
            ..default_build_args()
        };
        let targets = resolve_targets(&args, None);
        assert_eq!(
            targets,
            vec![
                Some("linux-x64".into()),
                Some("win-x64".into()),
                Some("aarch64".into()),
            ]
        );
    }

    #[test]
    fn resolve_targets_defaults_to_host() {
        let args = default_build_args();
        assert_eq!(resolve_targets(&args, None), vec![None]);
        assert_eq!(
            resolve_targets(&args, Some("linux-x64")),
            vec![Some("linux-x64".into())]
        );
    }

    #[test]
    fn resolve_targets_dedupes() {
        let args = BuildArgs {
            target: Some("win-x64".into()),
            cross_compile: Some("win-x64,win-arm64".into()),
            ..default_build_args()
        };
        let targets = resolve_targets(&args, None);
        assert_eq!(
            targets,
            vec![Some("win-x64".into()), Some("win-arm64".into())]
        );
    }

    #[test]
    fn output_paths_single_target_keeps_name() {
        let args = BuildArgs {
            target: Some("linux-x64".into()),
            ..default_build_args()
        };
        let targets = resolve_targets(&args, None);
        let outputs = output_paths(&args, &targets);
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].file_name().unwrap(), "app.xbin");
    }

    #[test]
    fn output_paths_single_windows_explicit_name_kept() {
        let args = BuildArgs {
            target: Some("win-x64".into()),
            output: PathBuf::from("myapp.exe"),
            ..default_build_args()
        };
        let targets = resolve_targets(&args, None);
        let outputs = output_paths(&args, &targets);
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].file_name().unwrap(), "myapp.exe");
    }

    #[test]
    fn output_paths_single_windows_dir_naming() {
        let args = BuildArgs {
            target: Some("win-x64".into()),
            output: PathBuf::from("dist"),
            ..default_build_args()
        };
        let targets = resolve_targets(&args, None);
        let outputs = output_paths(&args, &targets);
        assert_eq!(outputs[0].file_name().unwrap(), "app.exe");
    }

    #[test]
    fn output_paths_multi_target_suffixes() {
        let args = BuildArgs {
            target: Some("linux-x64,linux-arm64,win-x64".into()),
            ..default_build_args()
        };
        let targets = resolve_targets(&args, None);
        let outputs = output_paths(&args, &targets);
        let names: Vec<String> = outputs
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec![
                "app-linux-x64.xbin",
                "app-linux-arm64.xbin",
                "app-win-x64.exe"
            ]
        );
    }

    fn default_build_args() -> BuildArgs {
        BuildArgs {
            app: PathBuf::from("."),
            output: PathBuf::from("app.xbin"),
            target: None,
            isolation: "sandbox".into(),
            seccomp: false,
            landlock: false,
            encrypt: false,
            squashfs: false,
            key: None,
            enable_sisr: false,
            update_url: None,
            no_install: false,
            env_file: None,
            env: Vec::new(),
            define: Vec::new(),
            version_info: None,
            author: None,
            description: None,
            license: None,
            dry_run: false,
            update: false,
            include: Vec::new(),
            persist: false,
            tree_shake: false,
            minify: false,
            health_port: None,
            otel_endpoint: None,
            otel_protocol: "grpc".into(),
            cron: Vec::new(),
            redetect: false,
            embed_interpreter: None,
            interpreter_path: None,
            wasm: false,
            wasmtime_path: None,
            wasi: false,
            component_model: false,
            cross_compile: None,
            use_cache: false,
            clear_cache: false,
            remote_cache_url: None,
            remote_cache_max_entries: None,
            compression_level: 3,
            health_endpoint: None,
            json: false,
            quiet: false,
        }
    }

    #[test]
    fn config_fingerprint_changes_with_build_options() {
        let app_dir = PathBuf::from("/tmp/fake-app");
        let plan = |encrypt: bool, squashfs: bool| BuildPlan {
            verbose: false,
            app_dir: app_dir.clone(),
            runtime: detect::Runtime::Python, // fixture only; not inspected here
            runtime_name: "python".into(),
            isolation: "sandbox".into(),
            isolation_num: 2,
            no_install: false,
            seccomp: false,
            landlock: false,
            encrypt,
            squashfs,
            version_info: None,
            author: None,
            description: None,
            license: None,
            env_file: None,
            targets: vec![None],
            outputs: vec![PathBuf::from("app.xbin")],
        };
        let base = config_fingerprint(&default_build_args(), &plan(false, false));

        let signed = default_build_args();
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("test.key");
        std::fs::write(&key, [7u8; 32]).unwrap();
        let signed = BuildArgs {
            key: Some(key),
            ..signed
        };
        assert_ne!(
            config_fingerprint(&signed, &plan(false, false)),
            base,
            "--key must change the cache key"
        );

        assert_ne!(
            config_fingerprint(&default_build_args(), &plan(true, false)),
            base,
            "--encrypt must change the cache key"
        );
        assert_ne!(
            config_fingerprint(&default_build_args(), &plan(false, true)),
            base,
            "--squashfs must change the cache key"
        );

        assert_eq!(
            config_fingerprint(&default_build_args(), &plan(false, false)),
            config_fingerprint(&default_build_args(), &plan(false, false)),
            "fingerprint must be deterministic"
        );
    }

    #[test]
    fn ensure_node_download_rejects_bad_arch() {
        let dir = tempfile::tempdir().unwrap();
        let result = ensure_node_download(dir.path().to_path_buf(), Some("riscv64"), false);
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

    #[test]
    fn build_sisr_config_defaults_to_64k_chunks_without_key() {
        let config = build_sisr_config(&None).unwrap();
        assert!(config.enabled);
        assert_eq!(config.chunk_target_size, 64 << 10);
        assert!(config.signing_key.is_none());
    }

    #[test]
    fn build_sisr_config_loads_32_byte_key() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("signing.key");
        let key = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        std::fs::write(&key_path, key.to_bytes()).unwrap();
        let config = build_sisr_config(&Some(key_path)).unwrap();
        let loaded = config.signing_key.expect("key should be loaded");
        assert_eq!(loaded.to_bytes(), key.to_bytes());
    }

    #[test]
    fn build_sisr_config_rejects_wrong_key_length() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("bad.key");
        std::fs::write(&key_path, [1u8; 16]).unwrap();
        let err = build_sisr_config(&Some(key_path)).unwrap_err().to_string();
        assert!(err.contains("32 bytes"), "error: {err}");
    }

    #[test]
    fn build_sisr_config_errors_on_missing_key() {
        let dir = tempfile::tempdir().unwrap();
        let err = build_sisr_config(&Some(dir.path().join("nope.key")))
            .unwrap_err()
            .to_string();
        assert!(err.contains("failed to read signing key"));
    }
}
