//! Layer construction helpers — rootfs copy, /etc setup, filtered directory copy.

use std::fs;
use std::path::{Path, PathBuf};

pub fn copy_into_rootfs(host_path: &Path, rootfs: &Path) -> std::io::Result<()> {
    let rel = host_path
        .strip_prefix("/")
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| host_path.to_path_buf());
    let dest = rootfs.join(&rel);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    if dest.exists() || dest.is_symlink() {
        return Ok(());
    }
    if host_path.is_symlink() {
        let target = fs::read_link(host_path)?;
        let real = fs::canonicalize(host_path)?;
        copy_into_rootfs(&real, rootfs)?;
        match std::os::unix::fs::symlink(&target, &dest) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(()),
            Err(e) => return Err(e),
        }
        let expected = dest.parent().unwrap_or(rootfs).join(&target);
        let expected = fs::canonicalize(&expected).unwrap_or(expected);
        if !expected.exists() {
            let real_rel = real
                .strip_prefix("/")
                .map(Path::to_path_buf)
                .unwrap_or_else(|_| real.clone());
            let real_in_rootfs = rootfs.join(&real_rel);
            if real_in_rootfs.exists() {
                fs::remove_file(&dest)?;
                if let Some(parent) = dest.parent() {
                    let relpath =
                        pathdiff::diff_paths(&real_in_rootfs, parent).unwrap_or(real_in_rootfs);
                    std::os::unix::fs::symlink(&relpath, &dest)?;
                }
            }
        }
    } else {
        fs::copy(host_path, &dest)?;
    }
    Ok(())
}

pub fn write_etc(rootfs: &Path) -> std::io::Result<()> {
    let etc = rootfs.join("etc");
    fs::create_dir_all(&etc)?;
    fs::write(etc.join("passwd"), "root:x:0:0:root:/root:/bin/sh\n")?;
    fs::write(etc.join("group"), "root:x:0:\n")?;
    fs::write(etc.join("hosts"), "127.0.0.1 localhost\n::1 localhost\n")?;
    fs::write(etc.join("nsswitch.conf"), "hosts: files dns\n")?;
    fs::write(etc.join("resolv.conf"), "nameserver 1.1.1.1\n")?;
    Ok(())
}

pub fn copy_dir_recursive_filter(src: &Path, dst: &Path, skip: &[&str]) -> std::io::Result<()> {
    if !src.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("source is not a directory: {}", src.display()),
        ));
    }
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if skip.contains(&name_str.as_ref()) {
            continue;
        }
        let src_path = entry.path();
        let dst_path = dst.join(&name);
        if src_path.is_dir() {
            copy_dir_recursive_filter(&src_path, &dst_path, skip)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

pub fn build_cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs_home().join(".cache"));
    let d = base.join("xbin").join("build");
    fs::create_dir_all(&d).ok();
    d
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_copy_into_rootfs_regular_file() {
        let tmp = TempDir::new().unwrap();
        let rootfs = tmp.path().join("rootfs");
        fs::create_dir_all(&rootfs).unwrap();

        let host = tmp.path().join("usr").join("lib").join("libtest.so");
        fs::create_dir_all(host.parent().unwrap()).unwrap();
        fs::write(&host, b"binary content").unwrap();

        copy_into_rootfs(&host, &rootfs).unwrap();

        let rel = host.strip_prefix("/").unwrap();
        let dest = rootfs.join(rel);
        assert!(dest.exists());
        assert_eq!(fs::read(&dest).unwrap(), b"binary content");
    }

    #[test]
    fn test_copy_into_rootfs_symlink() {
        let tmp = TempDir::new().unwrap();
        let rootfs = tmp.path().join("rootfs");
        fs::create_dir_all(&rootfs).unwrap();

        let real = tmp.path().join("usr").join("lib").join("libreal.so");
        fs::create_dir_all(real.parent().unwrap()).unwrap();
        fs::write(&real, b"real").unwrap();

        let link_dir = tmp.path().join("lib");
        fs::create_dir_all(&link_dir).unwrap();
        let link = link_dir.join("libtest.so");
        std::os::unix::fs::symlink("../usr/lib/libreal.so", &link).unwrap();

        copy_into_rootfs(&link, &rootfs).unwrap();

        let rel = link.strip_prefix("/").unwrap();
        let dest = rootfs.join(rel);
        assert!(dest.is_symlink());
        assert!(fs::read_link(&dest).is_ok());
    }

    #[test]
    fn test_write_etc_creates_files() {
        let tmp = TempDir::new().unwrap();
        let rootfs = tmp.path().join("rootfs");
        fs::create_dir_all(&rootfs).unwrap();

        write_etc(&rootfs).unwrap();

        let etc = rootfs.join("etc");
        assert_eq!(
            fs::read_to_string(etc.join("passwd")).unwrap(),
            "root:x:0:0:root:/root:/bin/sh\n"
        );
        assert_eq!(
            fs::read_to_string(etc.join("group")).unwrap(),
            "root:x:0:\n"
        );
        assert_eq!(
            fs::read_to_string(etc.join("hosts")).unwrap(),
            "127.0.0.1 localhost\n::1 localhost\n"
        );
        assert_eq!(
            fs::read_to_string(etc.join("nsswitch.conf")).unwrap(),
            "hosts: files dns\n"
        );
        assert_eq!(
            fs::read_to_string(etc.join("resolv.conf")).unwrap(),
            "nameserver 1.1.1.1\n"
        );
    }

    #[test]
    fn test_copy_dir_recursive_filter_skips_dirs() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");

        // Create source structure
        fs::create_dir_all(src.join("keep")).unwrap();
        fs::write(src.join("keep").join("a.txt"), "a").unwrap();
        fs::create_dir_all(src.join(".git")).unwrap();
        fs::write(src.join(".git").join("config"), "x").unwrap();
        fs::create_dir_all(src.join("node_modules")).unwrap();
        fs::write(src.join("node_modules").join("pkg"), "").unwrap();
        fs::create_dir_all(src.join("__pycache__")).unwrap();
        fs::create_dir_all(src.join(".venv")).unwrap();
        fs::create_dir_all(src.join("venv")).unwrap();
        fs::create_dir_all(src.join(".xbin")).unwrap();
        fs::write(src.join("top.txt"), "top").unwrap();

        let skip = [
            ".git",
            "node_modules",
            "__pycache__",
            ".venv",
            "venv",
            ".xbin",
        ];
        copy_dir_recursive_filter(&src, &dst, &skip).unwrap();

        assert!(dst.join("keep").join("a.txt").exists());
        assert!(dst.join("top.txt").exists());
        assert!(!dst.join(".git").exists());
        assert!(!dst.join("node_modules").exists());
        assert!(!dst.join("__pycache__").exists());
        assert!(!dst.join(".venv").exists());
        assert!(!dst.join("venv").exists());
        assert!(!dst.join(".xbin").exists());
    }

    #[test]
    fn test_build_cache_dir_uses_xdg() {
        let tmp = TempDir::new().unwrap();
        let fake_cache = tmp.path().join("xdg_cache");
        std::env::set_var("XDG_CACHE_HOME", &fake_cache);

        let dir = build_cache_dir();
        assert_eq!(dir, fake_cache.join("xbin").join("build"));
        assert!(dir.exists());

        std::env::remove_var("XDG_CACHE_HOME");
    }

    #[test]
    fn test_build_cache_dir_fallback_home() {
        std::env::remove_var("XDG_CACHE_HOME");
        let home = std::env::var_os("HOME").map(PathBuf::from);

        let dir = build_cache_dir();
        if let Some(home) = home {
            assert_eq!(dir, home.join(".cache").join("xbin").join("build"));
        }
        assert!(dir.exists());
    }
}
