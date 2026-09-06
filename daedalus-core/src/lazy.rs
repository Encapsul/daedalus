//! Build-time lazy-loading priority expansion.
//!
//! Turns user-facing priority paths (`app-relative` like `main.py` or
//! payload-relative like `app/main.py`) into the exact tar entry names the
//! stub throws at `extract_atomic_lazy`. Directory arguments expand to every
//! file under them, mirroring the tar builder's own exclusions (`.git`,
//! `__pycache__`) so a priority can never reference an entry that is not
//! actually in the payload.

use std::io;
use std::path::{Component, Path};

/// Maximum number of priority entries in one build. Larger sets would bloat
/// metadata for no startup gain; go file-by-file instead.
pub const MAX_PRIORITY_FILES: usize = 2048;

/// Resolve user priority specs against the staged rootfs.
///
/// Each spec in `entries` is either app-relative (`main.py`) or
/// payload-relative (`app/main.py`). A spec naming a directory expands to all
/// files beneath it. Every resolved entry must exist inside `rootfs`, and the
/// returned list is sorted, deduplicated payload-relative path strings.
pub fn expand_priority_paths(entries: &[String], rootfs: &Path) -> io::Result<Vec<String>> {
    let mut resolved: Vec<String> = Vec::new();
    for spec in entries {
        let candidate = locate_spec(spec, rootfs)?;
        let ft = std::fs::symlink_metadata(&candidate)
            .map_err(|e| io::Error::new(io::ErrorKind::NotFound, format!("{spec}: {e}")))?;
        if ft.is_dir() {
            if !ft.file_type().is_symlink() {
                collect_dir(&candidate, rootfs, &mut resolved)?;
            }
        } else {
            resolved.push(relative_of(&candidate, rootfs)?);
        }
    }
    resolved.sort();
    resolved.dedup();
    if resolved.len() > MAX_PRIORITY_FILES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "priority set too large: {} files (max {MAX_PRIORITY_FILES}); list files instead of a directory",
                resolved.len()
            ),
        ));
    }
    Ok(resolved)
}

/// Find the on-disk candidate for a user spec, trying payload-relative first
/// when the spec already mentions `app/`, else app-relative first.
fn locate_spec(spec: &str, rootfs: &Path) -> io::Result<std::path::PathBuf> {
    let clean = spec.trim_start_matches('/');
    let attempts: Vec<std::path::PathBuf> = if clean.starts_with("app/") {
        vec![rootfs.join(clean)]
    } else {
        vec![rootfs.join("app").join(clean), rootfs.join(clean)]
    };
    for p in attempts {
        if p.exists() {
            return Ok(p);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("priority path not found in app or rootfs: {spec}"),
    ))
}

/// Recursively collect payload-relative paths under `current`.
///
/// Mirrors the tar builder: skips `.git`/`__pycache__` components and never
/// recurses through symlinked directories (cycle risk).
fn collect_dir(current: &Path, rootfs: &Path, out: &mut Vec<String>) -> io::Result<()> {
    let rd = std::fs::read_dir(current)?;
    for entry in rd {
        let entry = entry?;
        let path = entry.path();
        if path
            .components()
            .any(|c| matches!(c, Component::Normal(n) if n == ".git" || n == "__pycache__"))
        {
            continue;
        }
        let ft = entry.file_type()?;
        if ft.is_dir() && !ft.is_symlink() {
            collect_dir(&path, rootfs, out)?;
        } else {
            out.push(relative_of(&path, rootfs)?);
        }
    }
    Ok(())
}

/// Strip the rootfs prefix to get the payload-relative (tar) path string.
fn relative_of(path: &Path, rootfs: &Path) -> io::Result<String> {
    path.strip_prefix(rootfs)
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("prefix: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staged() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = tmp.path().join("rootfs");
        let app = rootfs.join("app");
        std::fs::create_dir_all(app.join("models")).unwrap();
        std::fs::write(app.join("app.py"), b"x").unwrap();
        std::fs::write(app.join("models/config.json"), b"{}").unwrap();
        std::fs::write(app.join("data.txt"), b"d").unwrap();
        std::fs::create_dir_all(app.join("__pycache__")).unwrap();
        std::fs::write(app.join("__pycache__/app.cpython-311.pyc"), b"y").unwrap();
        (tmp, rootfs)
    }

    #[test]
    fn expands_app_relative_files() {
        let (_tmp, rootfs) = staged();
        let entries = vec!["app.py".to_string(), "data.txt".to_string()];
        let files = expand_priority_paths(&entries, &rootfs).unwrap();
        assert_eq!(files, ["app/app.py", "app/data.txt"]);
    }

    #[test]
    fn expands_payload_relative_files() {
        let (_tmp, rootfs) = staged();
        let entries = vec!["app/models/config.json".to_string()];
        let files = expand_priority_paths(&entries, &rootfs).unwrap();
        assert_eq!(files, ["app/models/config.json"]);
    }

    #[test]
    fn expands_directories_recursively_skipping_pycache() {
        let (_tmp, rootfs) = staged();
        let entries = vec!["models".to_string()];
        let files = expand_priority_paths(&entries, &rootfs).unwrap();
        assert_eq!(files, ["app/models/config.json"]);
    }

    #[test]
    fn dedups_and_sorts() {
        let (_tmp, rootfs) = staged();
        let entries = vec!["app.py".to_string(), "app.py".to_string()];
        let files = expand_priority_paths(&entries, &rootfs).unwrap();
        assert_eq!(files, vec!["app/app.py"]);
    }

    #[test]
    fn errors_on_missing_path() {
        let (_tmp, rootfs) = staged();
        let entries = vec!["nope.py".to_string()];
        let err = expand_priority_paths(&entries, &rootfs).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }
}
