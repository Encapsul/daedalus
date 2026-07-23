use anyhow::{Context, Result};
use clap::Args;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const GITHUB_API: &str = "https://api.github.com/repos/Tednoob17/x.bin/releases/latest";

#[derive(Args)]
pub struct UpgradeArgs {
    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,
}

pub fn run(args: UpgradeArgs) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    if args.verbose {
        eprintln!("[xbin] current version: {current}");
    }

    let latest = fetch_latest_version().context("failed to fetch latest version")?;

    if args.verbose {
        eprintln!("[xbin] latest version:  {latest}");
    }

    if current == latest {
        eprintln!("[xbin] already up to date");
        return Ok(());
    }

    let platform = detect_platform()?;
    if args.verbose {
        eprintln!("[xbin] platform: {platform}");
    }

    let tag = format!("v{latest}");
    let asset = format!("xbin-{latest}-{platform}.tar.gz");
    let url = format!("https://github.com/Tednoob17/x.bin/releases/download/{tag}/{asset}");

    let tmp = tempfile::tempdir().context("failed to create temp dir")?;
    let tarball = tmp.path().join(&asset);

    // Download
    if args.verbose {
        eprintln!("[xbin] downloading {asset}...");
    }
    download_file(&url, &tarball).context("download failed")?;

    // Verify checksum
    let checksum_url = format!("{url}.sha256");
    match fetch_checksum(&checksum_url) {
        Ok(expected) => {
            let got = sha256_file(&tarball)?;
            if expected != got {
                anyhow::bail!("checksum mismatch: expected {expected}, got {got}");
            }
            if args.verbose {
                eprintln!("[xbin] checksum verified");
            }
        }
        Err(e) => {
            if args.verbose {
                eprintln!("[xbin] warning: could not verify checksum: {e}");
            }
        }
    }

    // Extract
    let status = std::process::Command::new("tar")
        .args([
            "xzf",
            &tarball.to_string_lossy(),
            "-C",
            &tmp.path().to_string_lossy(),
        ])
        .status()
        .context("failed to run tar")?;
    if !status.success() {
        anyhow::bail!("tar extraction failed");
    }

    // Find extracted directory
    let extracted = std::fs::read_dir(tmp.path())
        .context("failed to read temp dir")?
        .flatten()
        .find(|e| e.path().is_dir() && e.file_name().to_string_lossy().starts_with("xbin-"))
        .context("unexpected archive structure")?;

    let bin_dir = extracted.path().join("bin");
    if !bin_dir.is_dir() {
        anyhow::bail!("no bin/ directory in archive");
    }

    // Find install location
    let xbin_path = find_xbin_binary()?;
    let install_dir = xbin_path
        .parent()
        .context("cannot determine install directory")?;

    if args.verbose {
        eprintln!("[xbin] installing to {}...", install_dir.display());
    }

    for entry in std::fs::read_dir(&bin_dir).context("failed to read bin/ directory")? {
        let entry = entry?;
        let dest = install_dir.join(entry.file_name());
        if is_writable(&dest) {
            std::fs::copy(entry.path(), &dest)
                .with_context(|| format!("failed to copy to {}", dest.display()))?;
        } else {
            let status = std::process::Command::new("sudo")
                .args([
                    "cp",
                    &entry.path().to_string_lossy(),
                    &dest.to_string_lossy(),
                ])
                .status()
                .context("failed to run sudo cp")?;
            if !status.success() {
                anyhow::bail!("failed to install to {}", dest.display());
            }
        }
    }

    eprintln!("[xbin] upgraded to {latest}");
    Ok(())
}

fn detect_platform() -> Result<String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    let os_str = match os {
        "linux" => "linux",
        "macos" => "macos",
        _ => anyhow::bail!("unsupported OS: {os}"),
    };

    let arch_str = match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => anyhow::bail!("unsupported architecture: {arch}"),
    };

    Ok(format!("{os_str}-{arch_str}"))
}

fn fetch_latest_version() -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("failed to create HTTP client")?;

    let resp = client
        .get(GITHUB_API)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .context("failed to fetch latest release")?;

    let data: serde_json::Value = resp.json().context("failed to parse GitHub API response")?;
    let tag = data
        .get("tag_name")
        .and_then(|v: &serde_json::Value| v.as_str())
        .context("missing tag_name in response")?;

    Ok(tag.trim_start_matches('v').to_string())
}

fn download_file(url: &str, dest: &PathBuf) -> Result<()> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .context("failed to create HTTP client")?;

    let resp = client.get(url).send().context("failed to download")?;
    let bytes = resp.bytes().context("failed to read response")?;
    std::fs::write(dest, &bytes).context("failed to write file")?;
    Ok(())
}

fn fetch_checksum(url: &str) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("failed to create HTTP client")?;

    let resp = client.get(url).send().context("failed to fetch checksum")?;
    let text = resp.text().context("failed to read checksum")?;
    let hash = text
        .split_whitespace()
        .next()
        .context("empty checksum")?
        .to_string();
    Ok(hash)
}

fn sha256_file(path: &PathBuf) -> Result<String> {
    let bytes = std::fs::read(path).context("failed to read file for checksum")?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let result = hasher.finalize();
    Ok(hex::encode(result))
}

fn find_xbin_binary() -> Result<PathBuf> {
    // Try /proc/self/exe (Linux)
    let proc = PathBuf::from("/proc/self/exe");
    if proc.exists() {
        let target = std::fs::read_link(&proc).context("failed to read /proc/self/exe")?;
        return Ok(target);
    }

    // Try which
    if let Ok(p) = which::which("xbin") {
        return Ok(p);
    }

    anyhow::bail!("cannot locate xbin binary for self-update")
}

fn is_writable(path: &PathBuf) -> bool {
    if !path.exists() {
        return path
            .parent()
            .and_then(|p| std::fs::metadata(p).ok())
            .is_some_and(|m| !m.permissions().readonly());
    }
    std::fs::metadata(path).is_ok_and(|m| !m.permissions().readonly())
}
