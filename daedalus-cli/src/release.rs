//! Shared helpers for downloading daedalus release assets from GitHub.
//!
//! `cargo install daedalux` ships only the CLI; the stub and self-upgrade
//! command both fetch the matching (verified) release asset. Naming and
//! checksum handling must stay in sync with `.github/workflows/release.yml`:
//! `daedalus_<version>_<os>_<arch>.tar.gz` plus a single `checksums.txt`.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

const ASSET_BASE: &str = "https://github.com/Encapsul/daedalus/releases/download";
const MAX_TARBALL_BYTES: u64 = 200 * 1024 * 1024;

/// Release asset name for a version/tag pair, e.g. (`0.7.0`, darwin, amd64)
/// -> `daedalus_0.7.0_darwin_amd64.tar.gz`.
pub(crate) fn asset_name(version: &str, os_tag: &str, arch_tag: &str) -> String {
    format!("daedalus_{version}_{os_tag}_{arch_tag}.tar.gz")
}

/// Map a native arch suffix (e.g. `x86_64-apple-darwin`, `x86_64-pc-windows-gnu`)
/// to the `(os, arch)` tags used in release asset names. The release pipeline
/// only builds amd64/arm64 per OS, so exotic targets fail closed.
pub(crate) fn release_tags(arch_suffix: &str) -> Result<(String, String)> {
    let os = match arch_suffix.split('-').nth(1) {
        Some("unknown") => "linux",
        Some("apple") => "darwin",
        Some("pc") => "windows",
        _ => anyhow::bail!("no release asset for {arch_suffix}"),
    };
    let arch = match arch_suffix.split('-').next() {
        Some("x86_64") => "amd64",
        Some("aarch64") => "arm64",
        _ => anyhow::bail!("no release asset for {arch_suffix}"),
    };
    Ok((os.to_string(), arch.to_string()))
}

/// Download a release asset and verify its SHA-256 against `checksums.txt`.
/// The expected checksum is fetched BEFORE the payload so a release whose
/// integrity cannot be verified is never written to disk (fail closed).
pub(crate) fn download_release_asset(
    version: &str,
    arch_suffix: &str,
    verbose: bool,
) -> Result<Vec<u8>> {
    let (os, arch) = release_tags(arch_suffix)?;
    let name = asset_name(version, &os, &arch);
    let base = format!("{ASSET_BASE}/v{version}");
    let expected = fetch_checksum(&format!("{base}/checksums.txt"), &name)?;
    if verbose {
        eprintln!("[daedalus] expected checksum {expected}");
    }
    let bytes = download_verified(&format!("{base}/{name}"), &expected)?;
    Ok(bytes)
}

/// Fetch the SHA-256 for one asset from a release's `checksums.txt`.
fn fetch_checksum(checksums_url: &str, asset: &str) -> Result<String> {
    let text = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("daedalus/0.5")
        .build()
        .context("failed to build HTTP client")?
        .get(checksums_url)
        .send()
        .with_context(|| format!("failed to fetch {checksums_url}"))?
        .error_for_status()?
        .text()
        .context("failed to read checksums")?;

    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let hash = fields.next().context("malformed checksum line")?;
        if fields.any(|f| f.trim_start_matches('*') == asset) {
            return Ok(hash.to_string());
        }
    }
    anyhow::bail!("no checksum for {asset} in {checksums_url}")
}

/// Download a URL body with a size cap, verifying SHA-256 before returning.
fn download_verified(url: &str, expected: &str) -> Result<Vec<u8>> {
    let response = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_mins(5))
        .user_agent("daedalus/0.5")
        .build()
        .context("failed to build HTTP client")?
        .get(url)
        .send()
        .with_context(|| format!("failed to download {url}"))?
        .error_for_status()?;
    if let Some(len) = response.content_length() {
        if len > MAX_TARBALL_BYTES {
            anyhow::bail!("release asset too large: {len} bytes");
        }
    }

    let bytes = response
        .bytes()
        .context("failed to read response")?
        .to_vec();
    if bytes.len() as u64 > MAX_TARBALL_BYTES {
        anyhow::bail!("release asset too large: {} bytes", bytes.len());
    }

    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let got = hex::encode(hasher.finalize());
    if got != expected {
        anyhow::bail!("checksum mismatch for {url}: expected {expected}, got {got}");
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_tags_maps_suffixes() {
        assert_eq!(
            release_tags("x86_64-unknown-linux-musl").unwrap(),
            ("linux".into(), "amd64".into())
        );
        assert_eq!(
            release_tags("aarch64-apple-darwin").unwrap(),
            ("darwin".into(), "arm64".into())
        );
        assert_eq!(
            release_tags("x86_64-pc-windows-gnu").unwrap(),
            ("windows".into(), "amd64".into())
        );
    }

    #[test]
    fn unsupported_suffix_fails_closed() {
        assert!(release_tags("riscv64gc-unknown-linux-musl").is_err());
    }
}
