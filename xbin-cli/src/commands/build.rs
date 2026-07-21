use anyhow::{Context, Result};
use clap::Args;
use std::path::{Path, PathBuf};
use xbin_core::detect;
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

    /// Use SquashFS instead of zstd+tar
    #[arg(long)]
    pub squashfs: bool,

    /// Target architecture
    #[arg(long)]
    pub target: Option<String>,

    /// Skip dependency installation
    #[arg(long)]
    pub no_install: bool,

    /// Environment file to bake in
    #[arg(long)]
    pub env_file: Option<PathBuf>,

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
}

pub fn run(args: BuildArgs, verbose: bool) -> Result<()> {
    let app_dir = args.app.canonicalize().context("failed to canonicalize app path")?;
    if !app_dir.is_dir() {
        anyhow::bail!("{} is not a directory", app_dir.display());
    }

    let output = if args.output.extension().map_or(false, |e| e == "xbin") {
        args.output.clone()
    } else {
        args.output.join("app.xbin")
    };

    // Detect runtime
    let runtime = detect::detect_runtime(&app_dir);
    let runtime_name = match &runtime {
        Some(r) => r.name(),
        None => {
            anyhow::bail!(
                "could not detect runtime in {} — supported: python, node, deno, java, ruby, dotnet, go, php, perl, binary",
                app_dir.display()
            );
        }
    };

    if verbose {
        eprintln!("Detected runtime: {runtime_name}");
    }

    // Detect package manager
    let pkg_mgr = pkgmgr::detect_pkgmgr(&app_dir, runtime_name);
    if let Some(mgr) = &pkg_mgr {
        if verbose {
            eprintln!("Package manager: {}", mgr.name());
        }

        if !args.no_install {
            if verbose {
                eprintln!("Installing dependencies...");
            }
            let cmd = mgr.install_cmd();
            let status = std::process::Command::new(&cmd[0])
                .args(&cmd[1..])
                .current_dir(&app_dir)
                .status()
                .context("failed to run dependency installation command")?;
            if !status.success() {
                eprintln!("[xbin] warning: dependency installation failed");
            }
        }
    }

    // Find stub binary
    let stub = find_stub(&args.target)?;
    let stub_bytes = std::fs::read(&stub)
        .with_context(|| format!("failed to read stub binary at {}", stub.display()))?;

    // Create temp directory for layer building
    let tmp = tempfile::tempdir().context("failed to create temp directory")?;
    let rootfs = tmp.path().join("rootfs");
    std::fs::create_dir_all(&rootfs).context("failed to create rootfs directory")?;

    // Copy app files
    copy_dir_recursive(&app_dir, &rootfs.join("app"))
        .context("failed to copy app files")?;

    // Build deterministic tar
    if verbose {
        eprintln!("Creating payload...");
    }
    let tar_bytes = xbin_core::tar::create_deterministic_tar(&rootfs)
        .context("failed to create deterministic tar")?;
    let payload = xbin_core::compress::compress_with_level(&tar_bytes, 19)
        .context("failed to compress payload")?;

    // Build metadata
    let app_name = app_dir.file_name().map_or_else(
        || "app".to_string(),
        |n| n.to_string_lossy().into(),
    );

    let mut env_map = serde_json::Map::new();
    env_map.insert("XBIN_RUNTIME".into(), runtime_name.into());
    env_map.insert("XBIN_APP_NAME".into(), app_name.clone().into());

    let env_pairs: Vec<(String, String)> = env_map
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
        .collect();

    let isolation: u32 = args.isolation.parse().unwrap_or(1);

    let entrypoint = vec!["run".to_string()];

    let meta = xbin_core::assembly::build_meta_json(
        &app_name,
        runtime_name,
        isolation,
        &entrypoint,
        &env_pairs,
        &[],
        &xbin_core::assembly::MetaOptions {
            version: args.version_info,
            author: args.author,
            description: args.description,
            license: args.license,
            payload_format: Some("zstd-tar".to_string()),
            seccomp: args.seccomp,
            app_hash: None,
            rt_deps_hash: None,
        },
    );

    // Assemble
    if verbose {
        eprintln!("Assembling {}...", output.display());
    }

    let size = xbin_core::assembly::assemble_xbin(
        &output,
        &stub_bytes,
        &payload,
        &meta,
        args.encrypt,
        args.squashfs,
        args.target.as_deref(),
    )
    .context("failed to assemble xbin")?;

    eprintln!("Built {} ({:.1}MB)", output.display(), size as f64 / (1024.0 * 1024.0));

    // Sign if key provided
    if let Some(_key_path) = &args.key {
        eprintln!("Signing...");
        // Sign is handled by the sign command - for now skip
        eprintln!("  [xbin] note: use 'xbin sign' to sign the binary");
    }

    Ok(())
}

fn find_stub(target_arch: &Option<String>) -> Result<PathBuf> {
    // Try env var
    if let Ok(path) = std::env::var("XBIN_STUB_PATH") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    // Try cargo target dir
    let target_dir = std::env::var("CARGO_TARGET_DIR")
        .unwrap_or_else(|_| "target".into());

    let arch_suffix = match target_arch.as_deref() {
        Some("aarch64") => "aarch64-unknown-linux-musl",
        _ => "x86_64-unknown-linux-musl",
    };

    let candidates = [
        PathBuf::from(&target_dir).join(arch_suffix).join("release").join("xbin-stub"),
        PathBuf::from("stub/target").join(arch_suffix).join("release").join("xbin-stub"),
        PathBuf::from("/usr/local/bin/xbin-stub"),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            return Ok(candidate.clone());
        }
    }

    // Try PATH
    if let Ok(p) = which::which("xbin-stub") {
        return Ok(p);
    }

    anyhow::bail!("xbin-stub not found — run: make stub")
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        let name = src_path.file_name().unwrap_or_default().to_string_lossy();
        if name == ".git" || name == "node_modules" || name == "__pycache__"
            || name == ".venv" || name == "venv" || name == ".xbin"
        {
            continue;
        }

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
