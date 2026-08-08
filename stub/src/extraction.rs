//! Atomic extraction for the xbin launcher stub.
//!
//! Provides `extract_atomic` (zstd+tar) and `extract_squashfs_atomic`
//! (squashfs), both built on the shared `atomic_extract` helper.
//! All operations are atomic: extraction happens in a tmp directory and
//! is renamed into place only on success.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

use crate::nanos;

/// Extract zstd-compressed tar blobs atomically into `rootfs`.
pub fn extract_atomic(blobs: &[&[u8]], cache_root: &Path, rootfs: &Path) -> io::Result<()> {
    atomic_extract(cache_root, rootfs, |tmp_rootfs| {
        for blob in blobs {
            let mut decoder = zstd::Decoder::new(io::Cursor::new(*blob))
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("zstd: {e}")))?;
            let mut archive = tar::Archive::new(&mut decoder);
            archive.set_preserve_permissions(true);
            archive.set_overwrite(true);
            archive.unpack(tmp_rootfs)?;
        }
        Ok(())
    })
}

/// Extract squashfs blobs atomically into `rootfs`.
pub fn extract_squashfs_atomic(
    blobs: &[&[u8]],
    cache_root: &Path,
    rootfs: &Path,
) -> io::Result<()> {
    atomic_extract(cache_root, rootfs, |tmp_rootfs| {
        crate::squashfs_extract::extract_squashfs_layers(blobs, tmp_rootfs)
    })
}

/// Shared atomic extraction: create tmp dir, run extraction closure, write .ready, rename.
pub fn atomic_extract(
    cache_root: &Path,
    rootfs: &Path,
    extract_fn: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<()> {
    let parent = cache_root.parent().unwrap_or(Path::new("/tmp"));
    fs::create_dir_all(parent)?;

    let tmp = parent.join(format!(".tmp-{}-{}", std::process::id(), nanos()));
    let tmp_rootfs = tmp.join("rootfs");
    fs::create_dir_all(&tmp_rootfs)?;

    extract_fn(&tmp_rootfs)?;

    File::create(tmp.join(".ready"))?.write_all(b"1")?;

    match fs::rename(&tmp, cache_root) {
        Ok(()) => Ok(()),
        Err(_) if rootfs.exists() => {
            let _ = fs::remove_dir_all(&tmp);
            Ok(())
        }
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp);
            Err(e)
        }
    }
}
