use anyhow::{bail, Result};
use clap::Args;
use daedalus_core::detect;
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
#[allow(clippy::struct_excessive_bools)]
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
    pub(crate) cpu_limit: Option<u32>,
    pub(crate) memory_limit_mb: Option<u32>,
    pub(crate) pid_limit: Option<u32>,
    pub(crate) pre_hooks: Option<String>,
    pub(crate) post_hooks: Option<String>,
    pub(crate) squashfs: bool,
    pub(crate) version_info: Option<String>,
    pub(crate) author: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) license: Option<String>,
    pub(crate) env_file: Option<PathBuf>,
    pub(crate) targets: Vec<Option<String>>,
    pub(crate) outputs: Vec<PathBuf>,
    pub(crate) services: Vec<ServiceEntry>,
    pub(crate) entrypoint: Vec<String>,
}

/// A named service parsed from `--entrypoint name=cmd,arg1,...`.
#[derive(Debug, Clone)]
pub(crate) struct ServiceEntry {
    pub name: String,
    pub cmd: Vec<String>,
}

/// Parse raw `--entrypoint` flag values into a flat argv template and an
/// optional services list.
///
/// Backward-compatible flat syntax (no `=`):
///   `--entrypoint python3,main.py` -> `(["python3","main.py"], [])`
///   `--entrypoint python3`          -> `(["python3"], [])`
///
/// Multi-service syntax (`name=cmd,arg1,...`):
///   `--entrypoint api=python3,api.py` -> `([], [ServiceEntry{name:"api",cmd:["python3","api.py"]}])`
///
/// When any entrypoint uses the named syntax, the first service's `cmd` is
/// also returned as the flat entrypoint so older stubs that ignore `services`
/// still have something to execute.
pub(crate) fn parse_entrypoints(raw: &[String]) -> (Vec<String>, Vec<ServiceEntry>) {
    let mut flat = Vec::new();
    let mut services = Vec::new();
    for s in raw {
        if let Some((name, cmd_str)) = s.split_once('=') {
            let cmd: Vec<String> = cmd_str
                .split(',')
                .map(|part| part.trim().to_string())
                .filter(|part| !part.is_empty())
                .collect();
            services.push(ServiceEntry {
                name: name.trim().to_string(),
                cmd,
            });
        } else {
            let parts: Vec<String> = s
                .split(',')
                .map(|part| part.trim().to_string())
                .filter(|part| !part.is_empty())
                .collect();
            flat.extend(parts);
        }
    }
    if !services.is_empty() && flat.is_empty() {
        flat.clone_from(&services[0].cmd);
    }
    (flat, services)
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
/// (`-o app.de` stays `app.de`); multiple targets get a `<name>-<target>`
/// suffix so linux and windows artifacts never overwrite each other.
pub(crate) fn output_paths(args: &BuildArgs, targets: &[Option<String>]) -> Vec<PathBuf> {
    if targets.len() == 1 {
        let t = targets[0].as_deref();
        let is_windows_target = t.is_some_and(|t| parse_target(t).1 == "windows");
        let out = if args
            .output
            .extension()
            .is_some_and(|e| e == "daedalus" || e == "de" || e == "exe")
        {
            args.output.clone()
        } else {
            args.output.join(if is_windows_target {
                "app.exe"
            } else {
                "app.de"
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
            let ext = if is_windows { "exe" } else { "de" };
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
    canonical.push(format!("gui={}", plan.gui));
    canonical.push(format!("cpu_limit={:?}", plan.cpu_limit));
    canonical.push(format!("memory_limit_mb={:?}", plan.memory_limit_mb));
    canonical.push(format!("pid_limit={:?}", plan.pid_limit));
    canonical.push(format!("pre_hooks={:?}", plan.pre_hooks));
    canonical.push(format!("post_hooks={:?}", plan.post_hooks));
    canonical.push(format!("squashfs={}", plan.squashfs));
    canonical.push(format!("compress={}", args.compression_level));
    canonical.push(format!("sisr={}", args.enable_sisr));
    canonical.push(format!("redetect={}", args.redetect));
    if let Some(url) = &args.publish {
        canonical.push(format!("publish={url}"));
    }
    if let Some(url) = &args.update_url {
        canonical.push(format!("update_url={url}"));
    }
    canonical.push(format!("persist={}", args.persist));
    canonical.push(format!("health_port={:?}", args.health_port));
    if let Some(ep) = &args.health_endpoint {
        canonical.push(format!("health_endpoint={ep}"));
    }
    canonical.push(format!("wasm={}", args.wasm));
    if let Some(p) = &args.wasmtime_path {
        canonical.push(format!("wasmtime_path={}", p.display()));
    }
    canonical.push(format!("wasi={}", args.wasi));
    canonical.push(format!("component_model={}", args.component_model));
    if let Some(ref url) = args.electron_url {
        canonical.push(format!("electron_url={url}"));
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
    if let Some(ep) = &args.interpreter_path {
        canonical.push(format!("interpreter_path={ep}"));
    }
    canonical.push(format!("tree_shake={}", args.tree_shake));
    canonical.push(format!("minify={}", args.minify));
    let mut cron: Vec<&String> = args.cron.iter().collect();
    cron.sort();
    for c in cron {
        canonical.push(format!("cron={c}"));
    }
    let mut services: Vec<&String> = plan.services.iter().map(|s| &s.name).collect();
    services.sort();
    for s in services {
        canonical.push(format!("service={s}"));
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
  daedalus build ./myapp                         Build for the host (default output: app.de)
  daedalus build ./myapp -o myapp.de           Build for the host, custom output name
  daedalus build ./myapp --target linux-arm64 -o myapp.de     Single cross-target artifact
  daedalus build ./myapp --target linux-x64,linux-arm64 -o out/app.de  Multi-arch: emits app-linux-x64.de + app-linux-arm64.de
  daedalus build ./myapp --target win-x64 -o out/app.de       Cross-OS: Windows PE stub (.exe)
  daedalus build ./myapp --dry-run                              Preview the multi-target plan without building")]
#[allow(clippy::struct_excessive_bools)]
pub struct BuildArgs {
    /// Path to the app directory
    #[arg(default_value = ".")]
    pub app: PathBuf,

    /// Output file path
    #[arg(short, long, default_value = "app.de")]
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

    /// CPU limit (cgroup v2, Linux only). Percentage of a single CPU core.
    #[arg(long)]
    pub cpu_limit: Option<u32>,

    /// Memory limit in MB (cgroup v2, Linux only).
    #[arg(long)]
    pub memory_limit_mb: Option<u32>,

    /// Max number of processes (cgroup v2, Linux only).
    #[arg(long)]
    pub pid_limit: Option<u32>,

    /// Use `SquashFS` instead of zstd+tar
    #[arg(long)]
    pub squashfs: bool,

    /// Enable SISR delta-indexing and self-update support
    #[arg(long)]
    pub enable_sisr: bool,

    /// Encrypt the payload with AES-256-GCM using the key from this file (32 bytes hex).
    /// The decryption key must be provided at runtime via `--decrypt-key`.
    #[arg(long)]
    pub encrypt: Option<PathBuf>,

    /// Base URL of the SISR update channel
    #[arg(long)]
    pub update_url: Option<String>,

    /// Pre-extract hooks (JSON string or @file path)
    #[arg(long)]
    pub pre_hooks: Option<String>,

    /// Post-exec hooks (JSON string or @file path)
    #[arg(long)]
    pub post_hooks: Option<String>,

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

    /// Incremental rebuild — reuse unchanged layers from existing .daedalus
    #[arg(long)]
    pub update: bool,

    /// Include extra files/directories in the rootfs (repeatable).
    ///
    /// `.env` is excluded by default (secret-leak risk); pass
    /// `--include <app>/.env` explicitly to bundle it at your own risk.
    #[arg(long = "include", action = clap::ArgAction::Append)]
    pub include: Vec<PathBuf>,

    /// Enable persistent storage directory (`DAEDALUS_PERSIST_DIR`)
    #[arg(long)]
    pub persist: bool,

    /// Remove unused `node_modules` packages (tree-shaking)
    #[arg(long)]
    pub tree_shake: bool,

    /// Minify JS/TS/CSS files before packaging
    #[arg(long)]
    pub minify: bool,

    /// Health check HTTP port (sets `DAEDALUS_HEALTH_PORT`)
    #[arg(long)]
    pub health_port: Option<u16>,

    /// Scheduled task (repeatable): --cron NAME:SCHEDULE
    #[arg(long = "cron", action = clap::ArgAction::Append)]
    pub cron: Vec<String>,

    /// Force re-detection of dependencies (overwrite `daedalus.lock`)
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

    /// Enable WASM support with wasmtime
    #[arg(long)]
    pub wasm: bool,

    /// Path to wasmtime binary
    #[arg(long)]
    pub wasmtime_path: Option<PathBuf>,

    /// Custom mirror or directory for Electron runtime download.
    ///
    /// When set, overrides the default GitHub releases download URL.
    /// Can be a full URL prefix (e.g. `https://my-mirror.example.com/electron/`)
    /// or a local path to a pre-extracted Electron directory containing
    /// the `electron` binary.
    #[arg(long, env = "ELECTRON_URL")]
    pub electron_url: Option<String>,

    /// Enable WASI for WASM modules (WebAssembly System Interface)
    #[arg(long)]
    pub wasi: bool,

    /// Enable WASM component model support
    #[arg(long)]
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

    /// Generate daedalus-native bundle (default).
    ///
    /// Desktop packaging formats (flatpak, snap) have been removed in favor of
    /// the universal `.daedalus` binary format.
    #[arg(long, default_value = "daedalus")]
    pub _package_format: String,

    /// Publish layers to a content-addressable registry after successful build.
    /// Accepts a local directory path (for a filesystem-backed CAS) or a
    /// remote registry URL (HTTP/HTTPS with Bearer token auth).
    #[arg(long, env = "DAEDALUS_REGISTRY")]
    pub publish: Option<String>,

    /// Authentication token for the registry (when --publish uses HTTP/HTTPS)
    #[arg(long, env = "DAEDALUS_TOKEN")]
    pub token: Option<String>,

    /// Build a universal binary with multiple arch slices (`x86_64` + `aarch64` Linux).
    /// The output is a single `.daedalus` file with a polyglot shell-script
    /// launcher that detects `uname -m` and extracts the correct slice at runtime.
    #[arg(long)]
    pub universal: bool,

    /// Lazy-load the payload: extract priority files first, then continue
    /// extracting the rest in the background after the app starts.
    #[arg(long)]
    pub lazy_load: bool,
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
    let config_path = app_dir.join(".daedalus.toml");
    if !config_path.exists() {
        return XbinConfig::default();
    }
    match std::fs::read_to_string(&config_path) {
        Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
            eprintln!("[daedalus] warning: invalid .daedalus.toml: {e}");
            XbinConfig::default()
        }),
        Err(_) => XbinConfig::default(),
    }
}

/// Canonical `BuildArgs` for unit tests across the build module. Kept
/// test-only so the struct stays clap-constructed in production paths.
#[cfg(test)]
pub(crate) fn default_build_args() -> BuildArgs {
    BuildArgs {
        app: PathBuf::from("."),
        output: PathBuf::from("app.daedalus"),
        target: None,
        isolation: "sandbox".into(),
        seccomp: false,
        gui: false,
        cpu_limit: None,
        memory_limit_mb: None,
        pid_limit: None,
        pre_hooks: None,
        post_hooks: None,
        landlock: false,
        squashfs: false,
        key: None,
        enable_sisr: false,
        update_url: None,
        publish: None,
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
        json: false,
        quiet: false,
        _package_format: "daedalus".into(),
        entrypoint: Vec::new(),
        cron: Vec::new(),
        redetect: false,
        embed_interpreter: None,
        interpreter_path: None,
        wasm: false,
        wasmtime_path: None,
        wasi: false,
        component_model: false,
        electron_url: None,
        cross_compile: None,
        use_cache: false,
        clear_cache: false,
        remote_cache_url: None,
        remote_cache_max_entries: None,
        compression_level: 3,
        health_endpoint: None,
        token: None,
        universal: false,
        lazy_load: false,
        encrypt: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// sandbox_default_maps_to_level_2 - sandbox default maps to level 2.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn sandbox_default_maps_to_level_2() {
        assert_eq!(parse_isolation("sandbox").unwrap(), 2);
        assert_eq!(parse_isolation("2").unwrap(), 2);
    }

    #[test]
    /// none_maps_to_level_0 - none maps to level 0.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn none_maps_to_level_0() {
        assert_eq!(parse_isolation("none").unwrap(), 0);
        assert_eq!(parse_isolation("0").unwrap(), 0);
    }

    #[test]
    /// numeric_level_1_is_accepted - numeric level 1 is accepted.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn numeric_level_1_is_accepted() {
        assert_eq!(parse_isolation("1").unwrap(), 1);
    }

    #[test]
    /// invalid_values_fail_closed - invalid values fail closed.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn invalid_values_fail_closed() {
        assert!(parse_isolation("3").is_err());
        assert!(parse_isolation("root").is_err());
        assert!(parse_isolation("").is_err());
    }

    #[test]
    /// parse_target_short_forms - parse target short forms.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn parse_target_short_forms() {
        assert_eq!(parse_target("aarch64"), ("aarch64".into(), "linux".into()));
        assert_eq!(parse_target("x86_64"), ("x86_64".into(), "linux".into()));
    }

    #[test]
    /// parse_target_full_triples - parse target full triples.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// parse_target_windows_shorthands - parse target windows shorthands.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// resolve_targets_comma_list_and_cross_compile - resolve targets comma list and cross compile.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// resolve_targets_defaults_to_host - resolve targets defaults to host.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn resolve_targets_defaults_to_host() {
        let args = default_build_args();
        assert_eq!(resolve_targets(&args, None), vec![None]);
        assert_eq!(
            resolve_targets(&args, Some("linux-x64")),
            vec![Some("linux-x64".into())]
        );
    }

    #[test]
    /// resolve_targets_dedupes - resolve targets dedupes.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// output_paths_single_target_keeps_name - output paths single target keeps name.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn output_paths_single_target_keeps_name() {
        let args = BuildArgs {
            target: Some("linux-x64".into()),
            ..default_build_args()
        };
        let targets = resolve_targets(&args, None);
        let outputs = output_paths(&args, &targets);
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].file_name().unwrap(), "app.daedalus");
    }

    #[test]
    /// output_paths_single_windows_explicit_name_kept - output paths single windows explicit name kept.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// output_paths_single_windows_dir_naming - output paths single windows dir naming.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// output_paths_multi_target_suffixes - output paths multi target suffixes.
    ///
    /// Description:
    ///
    /// Return: nothing
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
            vec!["app-linux-x64.de", "app-linux-arm64.de", "app-win-x64.exe"]
        );
    }

    #[test]
    /// output_paths_single_ere_extension_kept - output paths single ere extension kept.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn output_paths_single_ere_extension_kept() {
        let args = BuildArgs {
            output: PathBuf::from("/tmp/hello-web.daedalus"),
            ..default_build_args()
        };
        let targets = resolve_targets(&args, None);
        let outputs = output_paths(&args, &targets);
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0], PathBuf::from("/tmp/hello-web.daedalus"));
    }

    #[test]
    /// config_fingerprint_changes_with_build_options - config fingerprint changes with build options.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn config_fingerprint_changes_with_build_options() {
        let app_dir = PathBuf::from("/tmp/fake-app");
        let plan = |squashfs: bool| BuildPlan {
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
            cpu_limit: None,
            memory_limit_mb: None,
            pid_limit: None,
            pre_hooks: None,
            post_hooks: None,
            squashfs,
            version_info: None,
            author: None,
            description: None,
            license: None,
            env_file: None,
            targets: vec![None],
            outputs: vec![PathBuf::from("app.daedalus")],
            services: Vec::new(),
            entrypoint: Vec::new(),
        };
        let base = config_fingerprint(&default_build_args(), &plan(false));

        let signed = default_build_args();
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("test.key");
        std::fs::write(&key, [7u8; 32]).unwrap();
        let signed = BuildArgs {
            key: Some(key),
            ..signed
        };
        assert_ne!(
            config_fingerprint(&signed, &plan(false)),
            base,
            "--key must change the cache key"
        );

        assert_ne!(
            config_fingerprint(&default_build_args(), &plan(true)),
            base,
            "--squashfs must change the cache key"
        );

        assert_eq!(
            config_fingerprint(&default_build_args(), &plan(false)),
            config_fingerprint(&default_build_args(), &plan(false)),
            "fingerprint must be deterministic"
        );
    }
}
