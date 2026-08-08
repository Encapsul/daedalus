//! File-tree walker that selects which files to include in the payload,
//! respecting `.xbinignore`, `.gitignore`, and built-in skip directories.
use std::fs;
use std::io;
use std::path::Path;

use sha2::{Digest, Sha256};

const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "__pycache__",
    ".venv",
    "venv",
    ".xbin",
    "dist",
    "build",
    ".next",
    ".nuxt",
    ".output",
    "coverage",
];

/// Copy extra files/directories into a destination directory.
///
/// Each source path is copied recursively. Directories are copied in full.
/// Skips `.git`, `node_modules`, etc.
pub fn copy_include_paths(sources: &[PathBuf], dest: &Path) -> io::Result<usize> {
    let mut count = 0;
    for src in sources {
        if !src.exists() {
            continue;
        }
        if src.is_dir() {
            copy_dir_recursive(src, dest)?;
        } else {
            let file_name = src.file_name().unwrap_or_default();
            fs::copy(src, dest.join(file_name))?;
        }
        count += 1;
    }
    Ok(count)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if SKIP_DIRS.contains(&name_str.as_ref()) {
            continue;
        }
        let src_path = entry.path();
        let dst_path = dst.join(&name);
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Compute a SHA-256 hash of all app files for change detection.
///
/// Excludes `.git`, `node_modules`, `.venv`, `venv`, `__pycache__`, `.xbin`.
pub fn hash_app_files(app_dir: &Path) -> String {
    let mut hasher = Sha256::new();
    let mut paths: Vec<_> = walk_dir_sorted(app_dir);
    paths.retain(|p| {
        if p.is_dir() {
            return false;
        }
        let parts: Vec<_> = p
            .strip_prefix(app_dir)
            .unwrap_or(p)
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        !parts.iter().any(|p| SKIP_DIRS.contains(&p.as_str()))
    });
    for p in &paths {
        if let Ok(bytes) = fs::read(p) {
            hasher.update(&bytes);
        }
    }
    hex::encode(hasher.finalize())
}

/// Compute a SHA-256 hash of the first matching lock file.
pub fn hash_lock_file(app_dir: &Path) -> String {
    const LOCK_FILES: &[&str] = &[
        "requirements.txt",
        "uv.lock",
        "poetry.lock",
        "Pipfile.lock",
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "bun.lockb",
    ];
    for name in LOCK_FILES {
        let p = app_dir.join(name);
        if p.is_file() {
            if let Ok(bytes) = fs::read(&p) {
                if !bytes.is_empty() {
                    return hex::encode(Sha256::digest(&bytes));
                }
            }
        }
    }
    String::new()
}

fn walk_dir_sorted(dir: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        if let Ok(entries) = fs::read_dir(&current) {
            let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
            entries.sort_by_key(|e| e.file_name());
            for entry in entries {
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    if SKIP_DIRS.contains(&name.as_ref()) {
                        continue;
                    }
                    stack.push(path.clone());
                }
                result.push(path);
            }
        }
    }
    result
}

use std::path::PathBuf;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_hash_app_files_deterministic() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("app.py"), "print('hello')").unwrap();
        let h1 = hash_app_files(dir.path());
        let h2 = hash_app_files(dir.path());
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_lock_file_empty() {
        let dir = TempDir::new().unwrap();
        assert_eq!(hash_lock_file(dir.path()), "");
    }

    #[test]
    fn test_hash_lock_file_requirements() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("requirements.txt"), "flask\n").unwrap();
        let h = hash_lock_file(dir.path());
        assert!(!h.is_empty());
    }

    #[test]
    fn test_copy_include_paths() {
        let src = TempDir::new().unwrap();
        let dst = TempDir::new().unwrap();
        fs::write(src.path().join("data.json"), "{}").unwrap();
        fs::create_dir_all(src.path().join("templates")).unwrap();
        fs::write(src.path().join("templates/a.html"), "<h1>hi</h1>").unwrap();

        let count = copy_include_paths(&[src.path().to_path_buf()], dst.path()).unwrap();
        assert_eq!(count, 1);
        assert!(dst.path().join("data.json").is_file());
    }
}
