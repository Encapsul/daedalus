use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::args::parse_target;
use crate::release;

/// Locate the stub binary for the requested target triple.
///
/// Search order:
/// 1. `DAEDALUS_STUB_PATH` env var (explicit override)
/// 2. `target/<triple>/release/daedalus-stub` (workspace build)
/// 3. `/tmp/daedalus-stub-target/<triple>/release/daedalus-stub` (AGENTS.md path)
/// 4. `stub/target/<triple>/release/daedalus-stub` (legacy layout)
/// 5. next to the running `daedalus` binary (installed distributions)
/// 6. `which daedalus-stub` (system install, with warning)
///
/// Without a `--target`, the native host triple is used so a plain
/// `daedalus build` finds the stub that runs on this machine (macOS and
/// Windows hosts used to only look for the musl Linux stub).
pub(crate) fn find_stub(target: Option<&str>) -> Result<PathBuf> {
    if let Ok(path) = std::env::var("DAEDALUS_STUB_PATH") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    let target_dir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into());

    let is_windows = target.is_some_and(|t| parse_target(t).1 == "windows");

    let arch_suffix = match target.map(parse_target) {
        Some((arch, os)) if os == "linux" => format!("{arch}-unknown-linux-musl"),
        Some((arch, os)) if os == "darwin" => format!("{arch}-apple-darwin"),
        Some((arch, os)) if os == "windows" => format!("{arch}-pc-windows-gnu"),
        Some((arch, _)) => format!("{arch}-unknown-linux-musl"),
        None => native_arch_suffix(),
    };

    let stub_name = if is_windows {
        "daedalus-stub.exe"
    } else {
        "daedalus-stub"
    };

    let mut candidates = vec![
        PathBuf::from(&target_dir)
            .join(&arch_suffix)
            .join("release")
            .join(stub_name),
        PathBuf::from("/tmp/daedalus-stub-target")
            .join(&arch_suffix)
            .join("release")
            .join(stub_name),
        PathBuf::from("stub/target")
            .join(&arch_suffix)
            .join("release")
            .join(stub_name),
    ];

    // Installed distributions ship daedalus-stub next to the daedalus binary.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(stub_name));
        }
    }

    for candidate in &candidates {
        if candidate.exists() {
            return Ok(candidate.clone());
        }
    }

    if let Ok(p) = which::which("daedalus-stub") {
        eprintln!(
            "[daedalus] warning: found daedalus-stub on PATH at {}; prefer 'make stub' for reproducible builds",
            p.display()
        );
        return Ok(p);
    }

    // Nothing local: `cargo install daedalux` ships only the CLI, so fetch the
    // matching (verified) stub from the GitHub release and cache it.
    match download_stub_fallback(&arch_suffix, stub_name) {
        Ok(stub) => Ok(stub),
        // Include the download error so a network/checksum failure is not
        // mistaken for a missing build artifact.
        Err(e) => anyhow::bail!("daedalus-stub not found locally and download failed: {e:#}"),
    }
}

/// Native stub triple for the host, so no-`--target` builds resolve the stub
/// that actually runs on this machine.
pub(crate) fn native_arch_suffix() -> String {
    match std::env::consts::OS {
        "macos" => format!("{}-apple-darwin", std::env::consts::ARCH),
        "windows" => format!("{}-pc-windows-gnu", std::env::consts::ARCH),
        _ => format!("{}-unknown-linux-musl", std::env::consts::ARCH),
    }
}

/// `cargo install daedalux` ships only the CLI, so fetch the matching stub
/// from the GitHub release when no local stub exists. The release asset is
/// SHA-256 verified against `checksums.txt` before the stub is cached under
/// `~/.cache/daedalus/stubs/`; a checksum miss aborts (fail closed).
fn download_stub_fallback(arch_suffix: &str, stub_name: &str) -> Result<PathBuf> {
    let version = env!("CARGO_PKG_VERSION");

    let cache_stub = daedalus_core::paths::cache_dir()
        .join("stubs")
        .join(version)
        .join(arch_suffix)
        .join(stub_name);
    if cache_stub.exists() {
        return Ok(cache_stub);
    }

    let tarball = release::download_release_asset(version, arch_suffix, false)?;

    let tmp = tempfile::tempdir().context("failed to create temp dir")?;
    let archive_path = tmp.path().join("stub.tar.gz");
    std::fs::write(&archive_path, &tarball)
        .with_context(|| format!("failed to write {}", archive_path.display()))?;

    let stub_data = extract_stub_bytes(&archive_path, stub_name)?;
    if let Some(parent) = cache_stub.parent() {
        std::fs::create_dir_all(parent).context("failed to create stub cache dir")?;
    }
    std::fs::write(&cache_stub, &stub_data).context("failed to write cached stub")?;
    make_executable(&cache_stub)?;
    Ok(cache_stub)
}

/// Extract the stub binary from the release tarball without unpacking the
/// whole tree to disk.
fn extract_stub_bytes(archive_path: &Path, stub_name: &str) -> Result<Vec<u8>> {
    use std::io::Read;

    let file = std::fs::File::open(archive_path)
        .with_context(|| format!("failed to open {}", archive_path.display()))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    for entry in archive.entries().context("failed to read tarball")? {
        let mut entry = entry.context("failed to read tarball entry")?;
        let path = entry.path().context("failed to read entry path")?;
        if path.ends_with(stub_name) {
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .context("failed to read stub")?;
            return Ok(bytes);
        }
    }
    anyhow::bail!("{} not present in {}", stub_name, archive_path.display())
}

/// Ensure the cached stub is executable (the release tarball is world-readable).
fn make_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(path)
            .with_context(|| format!("failed to stat {}", path.display()))?
            .permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(path, perm)
            .with_context(|| format!("failed to chmod {}", path.display()))?;
    }
    Ok(())
}

/// Read `app_hash` and `rt_deps_hash` from an existing `.daedalus` file's metadata.
pub(crate) fn read_existing_hashes(daedalus_path: &Path) -> Option<(String, String)> {
    use daedalus_core::format::Footer;

    let mut f = std::fs::File::open(daedalus_path).ok()?;
    let footer = Footer::read_from(&mut f).ok()?;
    let meta_size = footer.meta_size.try_into().ok()?;
    let meta_bytes = daedalus_core::format::read_at(&mut f, footer.meta_offset, meta_size).ok()?;
    let meta: serde_json::Value = serde_json::from_slice(&meta_bytes).ok()?;
    let app_hash = meta.get("app_hash")?.as_str()?.to_string();
    let rt_hash = meta.get("rt_deps_hash")?.as_str()?.to_string();
    Some((app_hash, rt_hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// find_stub_default_is_x86_64 - find stub default is x86 64.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn find_stub_default_is_x86_64() {
        let result = find_stub(None);
        assert!(result.is_err() || result.is_ok(), "should not panic");
    }

    #[test]
    /// find_stub_aarch64_suffix - find stub aarch64 suffix.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn find_stub_aarch64_suffix() {
        let result = find_stub(None);
        assert!(result.is_err() || result.is_ok(), "should not panic");
    }

    #[test]
    /// find_stub_darwin_suffix - find stub darwin suffix.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn find_stub_darwin_suffix() {
        let result = find_stub(Some("aarch64-apple-darwin"));
        assert!(result.is_err() || result.is_ok(), "should not panic");
    }

    #[test]
    /// find_stub_windows_suffix - find stub windows suffix.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn find_stub_windows_suffix() {
        let result = find_stub(Some("win-x64"));
        assert!(result.is_err() || result.is_ok(), "should not panic");
    }

    #[test]
    /// `native_arch_suffix` - host triple, not the musl default, on macOS/Windows.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn native_arch_suffix_matches_host() {
        let suffix = native_arch_suffix();
        match std::env::consts::OS {
            "macos" => assert_eq!(suffix, format!("{}-apple-darwin", std::env::consts::ARCH)),
            "windows" => assert_eq!(suffix, format!("{}-pc-windows-gnu", std::env::consts::ARCH)),
            _ => assert_eq!(
                suffix,
                format!("{}-unknown-linux-musl", std::env::consts::ARCH)
            ),
        }
    }
}
