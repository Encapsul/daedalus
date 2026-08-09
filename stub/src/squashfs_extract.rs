//! `squashfs` extraction — parse squashfs images and extract to a directory tree.
//!
//! Used by the v5 launcher path when `payload_format == "squashfs"`.
//! Each layer is an independent squashfs image. Layers are extracted sequentially
//! to the same target directory (later layers overwrite earlier ones, same as tar).

use std::fs;
use std::io::{self, Cursor, Read};
use std::path::{Component, Path, PathBuf};

use backhand::v4::filesystem::node::InnerNode;
use backhand::v4::filesystem::reader::FilesystemReader;

/// Maximum total decompressed bytes across all squashfs layers. Mirrors the
/// zstd+tar limit in `extraction.rs` so the default payload format (v5) gets
/// equivalent decompression-bomb defense.
const MAX_DECOMPRESSED_BYTES: u64 = 1024 * 1024 * 1024;
/// Maximum number of entries written across all squashfs layers.
const MAX_FILES: usize = 50_000;

/// Resolve `rel` under `dest`, rejecting any component that could escape it.
///
/// `rel` is expected to be a relative path (the leading `/` of a squashfs
/// fullpath is stripped beforehand). `ParentDir` (`..`) and any absolute prefix
/// are rejected so a crafted image cannot write outside `dest`. This is the
/// squashfs equivalent of the path protection the `tar` crate provides.
fn safe_join(dest: &Path, rel: &Path) -> io::Result<PathBuf> {
    let mut target = dest.to_path_buf();
    for comp in rel.components() {
        match comp {
            Component::CurDir => {}
            Component::Normal(part) => target.push(part),
            // RootDir / Prefix / ParentDir: cannot legitimately appear in a
            // relative path under `dest`. Any of them means the image tried to
            // escape the extraction root.
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("squashfs path escapes extraction root: {}", rel.display()),
                ));
            }
        }
    }
    Ok(target)
}

/// Extract a single squashfs image (as raw bytes) into `dest`.
///
/// Creates directories, writes files, and makes symlinks.
/// Permissions are preserved from the squashfs inode headers.
///
/// Decompression-bomb defense: per-entry size is capped before `read_to_end`
/// overflows memory, and running totals of bytes/files are bounded across all
/// layers (shared by the caller).
fn extract_squashfs_blob(
    blob: &[u8],
    dest: &Path,
    total_bytes: &mut u64,
    file_count: &mut usize,
) -> io::Result<()> {
    let cursor = Cursor::new(blob);
    let fs = FilesystemReader::from_reader(cursor)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("squashfs parse: {e}")))?;

    for node in fs.files() {
        let rel = node.fullpath.strip_prefix("/").unwrap_or(&node.fullpath);
        let target = safe_join(dest, rel)?;

        match &node.inner {
            InnerNode::Dir(_) => {
                fs::create_dir_all(&target)?;
            }
            InnerNode::File(file_reader) => {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                let reader = fs.file(file_reader).reader();
                // Bound the read so a malicious image cannot exhaust RAM before
                // we notice. `+1` lets us detect exact-capacity overshoot.
                let mut limited = reader.take(MAX_DECOMPRESSED_BYTES + 1);
                let mut buf = Vec::new();
                limited.read_to_end(&mut buf)?;
                let len = u64::try_from(buf.len()).unwrap_or(u64::MAX);
                if len > MAX_DECOMPRESSED_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "squashfs entry {} exceeds max size: {len} > {MAX_DECOMPRESSED_BYTES}",
                            rel.display()
                        ),
                    ));
                }
                *total_bytes = total_bytes.saturating_add(len);
                if *total_bytes > MAX_DECOMPRESSED_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("total decompressed size {total_bytes} exceeds {MAX_DECOMPRESSED_BYTES}"),
                    ));
                }
                *file_count = file_count.saturating_add(1);
                if *file_count > MAX_FILES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("file count exceeds max: {file_count} > {MAX_FILES}"),
                    ));
                }
                fs::write(&target, &buf)?;
                set_permissions(&target, node.header.permissions)?;
            }
            InnerNode::Symlink(sym) => {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                // Remove existing entry if a directory was created by a prior layer.
                if target.exists() || target.is_symlink() {
                    if target.is_dir() {
                        fs::remove_dir_all(&target)?;
                    } else {
                        fs::remove_file(&target)?;
                    }
                }
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(&sym.link, &target)?;
                }
                // Windows: copy the link target contents so the file is usable.
                #[cfg(windows)]
                {
                    let src = target.parent().unwrap_or(Path::new(".")).join(&sym.link);
                    if src.is_file() {
                        fs::copy(&src, &target)?;
                    }
                }
            }
            InnerNode::CharacterDevice(_)
            | InnerNode::BlockDevice(_)
            | InnerNode::NamedPipe
            | InnerNode::Socket => {
                // Skip device nodes, pipes, and sockets — not meaningful in
                // userspace extraction.
            }
        }
    }
    Ok(())
}

/// Extract multiple squashfs layer blobs sequentially into `dest`.
///
/// Later layers overwrite earlier ones (same semantics as tar layering).
/// Running byte/file totals are shared across layers and bounded here.
pub fn extract_squashfs_layers(blobs: &[&[u8]], dest: &Path) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    let mut total_bytes: u64 = 0;
    let mut file_count: usize = 0;
    for (i, blob) in blobs.iter().enumerate() {
        extract_squashfs_blob(blob, dest, &mut total_bytes, &mut file_count).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("squashfs layer {i}: {e}"),
            )
        })?;
    }
    Ok(())
}

/// Set file permissions from a squashfs mode (u16, POSIX mode bits).
/// No-op on Windows (no POSIX permissions).
#[cfg(unix)]
fn set_permissions(path: &Path, mode: u16) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(u32::from(mode));
    std::fs::set_permissions(path, perms)
}

#[cfg(windows)]
fn set_permissions(_path: &Path, _mode: u16) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_accepts_normal_relative_paths() {
        let dest = Path::new("/tmp/xbin_test_rootfs");
        let rel = Path::new("usr/bin/foo");
        let got = safe_join(dest, rel).unwrap();
        assert_eq!(got, dest.join("usr").join("bin").join("foo"));
    }

    #[test]
    fn safe_join_accepts_single_dot() {
        let dest = Path::new("/tmp/xbin_test_rootfs");
        let got = safe_join(dest, Path::new("a/./b")).unwrap();
        assert_eq!(got, dest.join("a").join("b"));
    }

    #[test]
    fn safe_join_rejects_parent_dir_traversal() {
        let dest = Path::new("/tmp/xbin_test_rootfs");
        let err = safe_join(dest, Path::new("a/../b")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn safe_join_rejects_absolute_escaped_path() {
        let dest = Path::new("/tmp/xbin_test_rootfs");
        // A relative path cannot contain RootDir; if one sneaks in, it is
        // rejected (no escape).
        let err = safe_join(dest, Path::new("/etc/passwd")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
