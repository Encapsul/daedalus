use anyhow::{bail, Result};
use clap::Args;
use erebus_core::detect;
use serde::Deserialize;
use sha2::Digest;
use std::path::{Path, PathBuf};

/// Map an isolation spec to the numeric level stored in metadata.
///
/// Levels: 0 = `LD_LIBRARY_PATH` (no sandbox), 1 = skipped, 2 = user +
/// mount namespaces with `pivot_root`. `sandbox` is the default and must
/// resolve to the implemented sandbox (2), not silently degrade.
pub(crate) fn parse_isolation(value: &str) -> Result<u32> {
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
pub(crate) fn parse_target(target: &str) -> (String, String) {
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
    } else {
        "linux"
    };
    (arch.to_string(), os.to_string())
}

/// Resolved, target-independent build settings shared by every target in a
/// multi-arch build. Kept separate from `BuildArgs` so the per-target build
/// loop takes a single struct instead of a parameter list.
pub(crate) struct BuildPlan {
    pub(crate) verbose: bool,
    pub(crate) app_dir: PathBuf,
    pub(crate) runtime: detect::Runtime,
    pub(crate) runtime_name: String,
    pub(crate) isolation: String,
    pub(crate) isolation_num: u32,
    pub(crate) no_install: bool,
    pub(crate) seccomp: bool,
    pub(crate) landlock: bool,
    pub(crate) gui: bool,
    pub(crate) encrypt: bool,
    pub(crate) squashfs: bool,
    pub(crate) version_info: Option<String>,
    pub(crate) author: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) license: Option<String>,
    pub(crate) env_file: Option<PathBuf>,
    pub(crate) targets: Vec<Option<String>>,
    pub(crate) outputs: Vec<PathBuf>,
}

/// Expand `--target`/`--cross-compile` (each comma-separated) plus the config
/// default into the ordered list of targets to build. `None` means "host
/// target". Duplicates are removed, first occurrence wins.
pub(crate) fn resolve_targets(
    args: &BuildArgs,
    config_target: Option<&str>,
) -> Vec<Option<String>> {
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
pub(crate) fn target_slug(target: Option<&str>) -> Option<String> {
    target.map(|t| t.replace(['/', '\\'], "-"))
}

/// One output path per target. A single target keeps the historical naming
/// (`-o app.erebus` stays `app.erebus`); multiple targets get a `<name>-<target>`
/// suffix so linux and windows artifacts never overwrite each other.
pub(crate) fn output_paths(args: &BuildArgs, targets: &[Option<String>]) -> Vec<PathBuf> {
    if targets.len() == 1 {
        let t = targets[0].as_deref();
        let is_windows_target = t.is_some_and(|t| parse_target(t).1 == "windows");
        let out = if args
            .output
            .extension()
            .is_some_and(|e| e == "erebus" || e == "exe")
        {
            args.output.clone()
        } else {
            args.output.join(if is_windows_target {
                "app.exe"
            } else {
                "app.erebus"
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
            let ext = if is_windows { "exe" } else { "erebus" };
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
pub(crate) fn config_fingerprint(args: &BuildArgs, plan: &BuildPlan) -> String {
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
    if args.embed_model.is_some() {
        canonical.push("embed_model=true".to_string());
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
  erebus build ./myapp                         Build for the host (default output: app.erebus)
  erebus build ./myapp -o myapp.erebus           Build for the host, custom output name
  erebus build ./myapp --target linux-arm64 -o myapp.erebus     Single cross-target artifact
  erebus build ./myapp --target linux-x64,linux-arm64 -o out/app.erebus  Multi-arch: emits app-linux-x64.erebus + app-linux-arm64.erebus
  erebus build ./myapp --target win-x64 -o out/app.erebus       Cross-OS: Windows PE stub (.exe)
  erebus build ./myapp --dry-run                              Preview the multi-target plan without building")]
pub struct BuildArgs {
    /// Path to the app directory
    #[arg(default_value = ".")]
    pub app: PathBuf,

    /// Output file path
    #[arg(short, long, default_value = "app.erebus")]
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

    /// GUI app — bind-mount X11/Wayland/GPU before `pivot_root`
    #[arg(long)]
    pub gui: bool,

    /// Enable Landlock LSM filesystem sandbox
    #[arg(long)]
    pub landlock: bool,

    /// Encrypt the payload with AES-256-GCM (requires --key).
    ///
    /// WARNING: this provides obfuscation against casual inspection only, NOT
    /// confidentiality. The AES key is stored in the binary's metadata next to
    /// the ciphertext, so anyone holding the `.erebus` can decrypt it. Real
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

    /// Incremental rebuild — reuse unchanged layers from existing .erebus
    #[arg(long)]
    pub update: bool,

    /// Include extra files/directories in the rootfs (repeatable).
    ///
    /// `.env` is excluded by default (secret-leak risk); pass
    /// `--include <app>/.env` explicitly to bundle it at your own risk.
    #[arg(long = "include", action = clap::ArgAction::Append)]
    pub include: Vec<PathBuf>,

    /// Enable persistent storage directory (`ERE_PERSIST_DIR`)
    #[arg(long)]
    pub persist: bool,

    /// Remove unused `node_modules` packages (tree-shaking)
    #[arg(long)]
    pub tree_shake: bool,

    /// Minify JS/TS/CSS files before packaging
    #[arg(long)]
    pub minify: bool,

    /// Health check HTTP port (sets `ERE_HEALTH_PORT`)
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

    /// Force re-detection of dependencies (overwrite `erebus.lock`)
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

    /// Override entrypoint command (repeatable): --entrypoint python3,main.py
    #[arg(long = "entrypoint", action = clap::ArgAction::Append)]
    pub entrypoint: Vec<String>,

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

    /// Path to a large model file (e.g., .gguf). Tunes `FastCDC` chunk size to
    /// 16 MiB for efficient SISR delta updates on multi-GB model payloads.
    #[arg(long)]
    pub embed_model: Option<PathBuf>,

    /// Generate desktop packaging files (flatpak: flatpak-manifest.json + .desktop,
    /// snap: snapcraft.yaml) alongside the erebus bundle
    #[arg(long, value_parser = ["erebus", "flatpak", "snap"], default_value = "erebus")]
    pub package_format: String,
}

#[derive(Default, Deserialize)]
pub(crate) struct XbinConfig {
    #[serde(default)]
    pub(crate) build: BuildConfig,
    #[serde(default)]
    pub(crate) package: PackageConfig,
}

#[derive(Default, Deserialize)]
pub(crate) struct BuildConfig {
    pub isolation: Option<String>,
    pub seccomp: Option<bool>,
    pub landlock: Option<bool>,
    pub gui: Option<bool>,
    pub encrypt: Option<bool>,
    pub squashfs: Option<bool>,
    pub target: Option<String>,
    pub no_install: Option<bool>,
    pub env_file: Option<String>,
}

#[derive(Default, Deserialize)]
pub(crate) struct PackageConfig {
    pub version: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
}

pub(crate) fn load_config(app_dir: &Path) -> XbinConfig {
    let config_path = app_dir.join(".erebus.toml");
    if !config_path.exists() {
        return XbinConfig::default();
    }
    match std::fs::read_to_string(&config_path) {
        Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
            eprintln!("[erebus] warning: invalid .erebus.toml: {e}");
            XbinConfig::default()
        }),
        Err(_) => XbinConfig::default(),
    }
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
        assert_eq!(outputs[0].file_name().unwrap(), "app.erebus");
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
                "app-linux-x64.erebus",
                "app-linux-arm64.erebus",
                "app-win-x64.exe"
            ]
        );
    }

    fn default_build_args() -> BuildArgs {
        BuildArgs {
            app: PathBuf::from("."),
            output: PathBuf::from("app.erebus"),
            target: None,
            isolation: "sandbox".into(),
            seccomp: false,
            gui: false,
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
            embed_model: None,
            package_format: "erebus".into(),
            entrypoint: Vec::new(),
        }
    }

    #[test]
    fn config_fingerprint_changes_with_build_options() {
        let app_dir = PathBuf::from("/tmp/fake-app");
        let plan = |encrypt: bool, squashfs: bool| BuildPlan {
            verbose: false,
            app_dir: app_dir.clone(),
            runtime: detect::Runtime::Python,
            runtime_name: "python".into(),
            isolation: "sandbox".into(),
            isolation_num: 2,
            no_install: false,
            seccomp: false,
            gui: false,
            landlock: false,
            encrypt,
            squashfs,
            version_info: None,
            author: None,
            description: None,
            license: None,
            env_file: None,
            targets: vec![None],
            outputs: vec![PathBuf::from("app.erebus")],
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
}
