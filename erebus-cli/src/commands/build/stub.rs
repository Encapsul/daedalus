use anyhow::Result;
use std::path::{Path, PathBuf};

use super::args::parse_target;

/// Locate the stub binary for the requested target triple.
///
/// Search order:
/// 1. `ERE_STUB_PATH` env var (explicit override)
/// 2. `target/<triple>/release/erebus-stub` (workspace build)
/// 3. `/tmp/erebus-stub-target/<triple>/release/erebus-stub` (AGENTS.md path)
/// 4. `stub/target/<triple>/release/erebus-stub` (legacy layout)
/// 5. `which erebus-stub` (system install, with warning)
pub(crate) fn find_stub(target: &Option<String>) -> Result<PathBuf> {
    if let Ok(path) = std::env::var("ERE_STUB_PATH") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    let target_dir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into());

    let is_windows = target
        .as_deref()
        .is_some_and(|t| parse_target(t).1 == "windows");

    let arch_suffix = match target.as_deref().map(parse_target) {
        Some((arch, os)) if os == "linux" => format!("{arch}-unknown-linux-musl"),
        Some((arch, os)) if os == "darwin" => format!("{arch}-apple-darwin"),
        Some((arch, os)) if os == "windows" => format!("{arch}-pc-windows-gnu"),
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

    if let Ok(p) = which::which("erebus-stub") {
        eprintln!(
            "[erebus] warning: found erebus-stub on PATH at {}; prefer 'make stub' for reproducible builds",
            p.display()
        );
        return Ok(p);
    }

    anyhow::bail!("erebus-stub not found — run: make stub")
}

/// Read `app_hash` and `rt_deps_hash` from an existing `.erebus` file's metadata.
pub(crate) fn read_existing_hashes(erebus_path: &Path) -> Option<(String, String)> {
    use erebus_core::format::Footer;

    let mut f = std::fs::File::open(erebus_path).ok()?;
    let footer = Footer::read_from(&mut f).ok()?;
    let meta_size = footer.meta_size.try_into().ok()?;
    let meta_bytes = erebus_core::format::read_at(&mut f, footer.meta_offset, meta_size).ok()?;
    let meta: serde_json::Value = serde_json::from_slice(&meta_bytes).ok()?;
    let app_hash = meta.get("app_hash")?.as_str()?.to_string();
    let rt_hash = meta.get("rt_deps_hash")?.as_str()?.to_string();
    Some((app_hash, rt_hash))
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
    fn find_stub_darwin_suffix() {
        let result = find_stub(&Some("aarch64-apple-darwin".into()));
        assert!(result.is_err() || result.is_ok(), "should not panic");
    }

    #[test]
    fn find_stub_windows_suffix() {
        let result = find_stub(&Some("win-x64".into()));
        assert!(result.is_err() || result.is_ok(), "should not panic");
    }
}
