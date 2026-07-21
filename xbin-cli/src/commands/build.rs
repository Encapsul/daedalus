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

    /// Use `SquashFS` instead of zstd+tar
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

    /// Dry run — show what would be built without building
    #[arg(long)]
    pub dry_run: bool,
}

pub fn run(args: BuildArgs, verbose: bool) -> Result<()> {
    let app_dir = args.app.canonicalize().context("failed to canonicalize app path")?;
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
        if let Some(ref v) = version_info { eprintln!("  Version:   {v}"); }
        if let Some(ref a) = author { eprintln!("  Author:    {a}"); }
        if let Some(ref d) = description { eprintln!("  Desc:      {d}"); }
        if let Some(ref l) = license { eprintln!("  License:   {l}"); }
        if let Some(ref t) = target { eprintln!("  Target:    {t}"); }
        if let Some(ref e) = env_file { eprintln!("  Env file:  {}", e.display()); }
        if no_install { eprintln!("  No install: yes"); }

        // Detect package manager
        let pkg_mgr = pkgmgr::detect_pkgmgr(&app_dir, runtime_name);
        if let Some(mgr) = &pkg_mgr {
            eprintln!("  Pkg mgr:   {}", mgr.name());
            if !no_install {
                let cmd = mgr.install_cmd();
                eprintln!("  Install:   {}", cmd.join(" "));
            }
        } else {
            eprintln!("  Pkg mgr:   (none)");
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

    // Detect package manager
    let pkg_mgr = pkgmgr::detect_pkgmgr(&app_dir, runtime_name);
    if let Some(mgr) = &pkg_mgr {
        if verbose {
            eprintln!("Package manager: {}", mgr.name());
        }

        if !no_install {
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
    let stub = find_stub(&target)?;
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

    let entrypoint = vec!["run".to_string()];

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
        encrypt,
        squashfs,
        target.as_deref(),
    )
    .context("failed to assemble xbin")?;

    eprintln!("Built {} ({:.1}MB)", output.display(), size as f64 / (1024.0 * 1024.0));

    // Sign if key provided
    if let Some(_key_path) = &args.key {
        eprintln!("Signing...");
        eprintln!("  [xbin] note: use 'xbin sign' to sign the binary");
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

fn find_stub(target_arch: &Option<String>) -> Result<PathBuf> {
    if let Ok(path) = std::env::var("XBIN_STUB_PATH") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

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

    if let Ok(p) = which::which("xbin-stub") {
        return Ok(p);
    }

    anyhow::bail!("xbin-stub not found — run: make stub")
}

fn count_files(dir: &Path) -> usize {
    let mut count = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str == ".git" || name_str == "node_modules" || name_str == "__pycache__"
                || name_str == ".venv" || name_str == "venv" || name_str == ".xbin"
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
            if name_str == ".git" || name_str == "node_modules" || name_str == "__pycache__"
                || name_str == ".venv" || name_str == "venv" || name_str == ".xbin"
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
