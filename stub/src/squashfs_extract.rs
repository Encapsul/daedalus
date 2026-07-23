//! `squashfs` extraction — parse squashfs images and extract to a directory tree.
//!
//! Used by the v5 launcher path when `payload_format == "squashfs"`.
//! Each layer is an independent squashfs image. Layers are extracted sequentially
//! to the same target directory (later layers overwrite earlier ones, same as tar).

use std::fs;
use std::io::{self, Cursor, Read};
use std::path::Path;

use backhand::v4::filesystem::node::InnerNode;
use backhand::v4::filesystem::reader::FilesystemReader;

/// Extract a single squashfs image (as raw bytes) into `dest`.
///
/// Creates directories, writes files, and makes symlinks.
/// Permissions are preserved from the squashfs inode headers.
pub fn extract_squashfs_blob(blob: &[u8], dest: &Path) -> io::Result<()> {
    let cursor = Cursor::new(blob);
    let fs = FilesystemReader::from_reader(cursor)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("squashfs parse: {e}")))?;

    for node in fs.files() {
        let rel = node.fullpath.strip_prefix("/").unwrap_or(&node.fullpath);
        let target = dest.join(rel);

        match &node.inner {
            InnerNode::Dir(_) => {
                fs::create_dir_all(&target)?;
            }
            InnerNode::File(file_reader) => {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut reader = fs.file(file_reader).reader();
                let mut buf = Vec::new();
                reader.read_to_end(&mut buf)?;
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
                std::os::unix::fs::symlink(&sym.link, &target)?;
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
pub fn extract_squashfs_layers(blobs: &[&[u8]], dest: &Path) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    for (i, blob) in blobs.iter().enumerate() {
        extract_squashfs_blob(blob, dest)
            .map_err(|e| io::Error::new(e.kind(), format!("squashfs layer {i}: {e}")))?;
    }
    Ok(())
}

/// Set file permissions from a squashfs mode (u16, POSIX mode bits).
fn set_permissions(path: &Path, mode: u16) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(u32::from(mode));
    std::fs::set_permissions(path, perms)
}
