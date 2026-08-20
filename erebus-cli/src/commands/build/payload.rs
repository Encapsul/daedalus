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
                || name_str == ".erebus"
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
                || name_str == ".erebus"
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
            || name == ".erebus"
            || name == ".pnpm"
            || name == ".env"
            || (name == "node_modules" && !include_node_modules)
        {
            continue;
        }

        if src_path.is_dir() {
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
            "mksquashfs not found on PATH — install squashfs-tools to build \
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
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
            &[dir.path().join("sub").join("erebus.toml")]
        ));
    }
}
