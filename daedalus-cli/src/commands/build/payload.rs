use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::deps::is_command_available;

/// Count files in a directory tree, excluding common non-essential directories.
pub(crate) fn count_files(dir: &Path) -> usize {
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
                || name_str == ".daedalus"
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

/// Print a directory tree to stderr, excluding common non-essential directories.
pub(crate) fn print_tree(dir: &Path, indent: usize) {
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
                || name_str == ".daedalus"
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

/// Recursively copy a directory, optionally excluding `node_modules`.
pub(crate) fn copy_dir_recursive_with(
    src: &Path,
    dst: &Path,
    include_node_modules: bool,
) -> Result<()> {
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
            || name == ".daedalus"
            || name == ".pnpm"
            || name == ".env"
            || (name == "node_modules" && !include_node_modules)
        {
            continue;
        }

        let metadata = entry.metadata()?;
        #[cfg(unix)]
        if metadata.file_type().is_symlink() {
            if let Ok(target) = std::fs::read_link(&src_path) {
                let _ = std::os::unix::fs::symlink(&target, &dst_path);
            }
            continue;
        }
        if metadata.is_dir() {
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
pub(crate) fn include_points_to_env(app_dir: &Path, includes: &[PathBuf]) -> bool {
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
pub(crate) fn create_squashfs_payload(rootfs: &Path, verbose: bool) -> Result<Vec<u8>> {
    if !is_command_available("mksquashfs") {
        anyhow::bail!(
            "mksquashfs not found on PATH - install squashfs-tools to build \
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

/// Apply URL-userinfo redaction to an arbitrary text blob.
pub(crate) fn redact_urls(text: &str) -> String {
    text.split_inclusive('\n')
        .map(redact_url_userinfo)
        .collect()
}

/// Strip credentials embedded in URL userinfo (`https://user:token@host/...`).
/// The scheme and host stay intact so public-index installs keep working.
fn redact_url_userinfo(line: &str) -> String {
    let Some(scheme) = line.find("://") else {
        return line.to_string();
    };
    let auth_start = scheme + 3;
    let rest = &line[auth_start..];
    let auth_end = rest
        .find(['/', '?', '#', ' ', '\t', '"', '\'', '\n', '\r'])
        .unwrap_or(rest.len());
    let authority = &rest[..auth_end];
    let Some(at) = authority.rfind('@') else {
        return line.to_string();
    };
    if at == 0 {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len());
    out.push_str(&line[..auth_start]);
    out.push_str("***@");
    out.push_str(&authority[at + 1..]);
    out.push_str(&rest[auth_end..]);
    out
}

/// Whether a staged file may carry dependency URLs worth redacting.
fn redactable_manifest(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let lower = name.to_ascii_lowercase();
    lower == "pipfile"
        || lower.starts_with("requirements")
        || [
            ".txt", ".toml", ".lock", ".cfg", ".ini", ".json", ".yaml", ".yml",
        ]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// Remove pip's provenance files and scrub private URL credentials before the
/// staged tree is embedded. `direct_url.json` often records the original
/// authenticated index URL, and lockfiles can contain the same token.
pub(crate) fn redact_private_urls(root: &Path) -> Result<()> {
    let max_size = 4 << 20;
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|e| anyhow::anyhow!("failed to walk staged rootfs: {e}"))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == "direct_url.json" {
            std::fs::remove_file(path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
            continue;
        }
        if !redactable_manifest(path) || std::fs::metadata(path)?.len() > max_size {
            continue;
        }
        let Ok(original) = std::fs::read_to_string(path) else {
            continue;
        };
        let redacted = redact_urls(&original);
        if redacted != original {
            std::fs::write(path, redacted)
                .with_context(|| format!("failed to redact {}", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redaction_strips_userinfo_but_keeps_public_urls() {
        let input = "https://alice:s3cr3t@private.example/simple \\\n    --index-url https://user:token@pypi.example/simple\nhttps://pypi.org/simple\n";
        let out = redact_urls(input);
        assert!(!out.contains("s3cr3t"));
        assert!(!out.contains("user:token"));
        assert!(out.contains("https://***@private.example/simple"));
        assert!(out.contains("https://pypi.org/simple"));
    }

    #[test]
    fn redaction_removes_direct_url_provenance_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let dist = dir.path().join("site-packages/foo-1.0.dist-info");
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(
            dist.join("direct_url.json"),
            b"{\"url\":\"https://u:p@x/\"}",
        )
        .unwrap();
        std::fs::write(
            dist.join("requirements.txt"),
            "https://u:p@private.example/a.whl\n",
        )
        .unwrap();
        redact_private_urls(dir.path()).unwrap();
        assert!(!dist.join("direct_url.json").exists());
        let req = std::fs::read_to_string(dist.join("requirements.txt")).unwrap();
        assert!(!req.contains("u:p@"));
        assert!(req.contains("https://***@private.example/a.whl"));
    }

    #[test]
    /// include_points_to_env_matches_only_explicit_env - include points to env matches only explicit env.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn include_points_to_env_matches_only_explicit_env() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join(".env"), "SECRET=1").unwrap();
        assert!(!include_points_to_env(dir.path(), &[]));
        assert!(include_points_to_env(
            dir.path(),
            &[dir.path().join(".env")]
        ));
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        assert!(include_points_to_env(
            dir.path(),
            &[dir.path().join("sub").join("../.env")]
        ));
        assert!(!include_points_to_env(
            dir.path(),
            &[dir.path().join("sub").join("daedalus.toml")]
        ));
    }
}
