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

/// Maximum total decompressed bytes across all tar entries (1 GB).
const MAX_DECOMPRESSED_BYTES: u64 = 1024 * 1024 * 1024;
/// Maximum number of files in a single tar archive.
const MAX_FILES: usize = 50_000;

/// Extract zstd-compressed tar blobs atomically into `rootfs`.
pub fn extract_atomic(blobs: &[&[u8]], cache_root: &Path, rootfs: &Path) -> io::Result<()> {
    atomic_extract(cache_root, rootfs, |tmp_rootfs| {
        let mut total_bytes: u64 = 0;
        let mut file_count: usize = 0;
        for blob in blobs {
            let mut decoder = zstd::Decoder::new(io::Cursor::new(*blob))
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("zstd: {e}")))?;
            let mut archive = tar::Archive::new(&mut decoder);
            archive.set_preserve_permissions(true);
            archive.set_overwrite(true);

            // Iterate entries manually to enforce size + file-count limits
            // (decompression-bomb defense).  `tar::unpack` alone has no limits.
            for entry in archive.entries()? {
                let mut entry = entry?;
                let size = entry.size();
                if size > MAX_DECOMPRESSED_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("tar entry exceeds max size: {size} > {MAX_DECOMPRESSED_BYTES}"),
                    ));
                }
                total_bytes = total_bytes.saturating_add(size);
                if total_bytes > MAX_DECOMPRESSED_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("total decompressed size {total_bytes} exceeds {MAX_DECOMPRESSED_BYTES}"),
                    ));
                }
                file_count = file_count.saturating_add(1);
                if file_count > MAX_FILES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("file count exceeds max: {file_count} > {MAX_FILES}"),
                    ));
                }
                entry.unpack_in(tmp_rootfs)?;
            }
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

    // Only treat an existing rootfs as valid if it has a .ready marker.
    // This prevents using a stale/partial extraction as a fallback, which
    // would mask real errors (full disk, permissions, etc.).
    let marker = rootfs.join(".ready");
    match fs::rename(&tmp, cache_root) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp);
            // If the rootfs has a valid .ready marker, the previous extraction
            // is still usable — surface the error but don't block the run.
            if marker.exists() {
                eprintln!("[xbin] warning: cache rename failed but existing rootfs is valid: {e}");
                Ok(())
            } else {
                Err(e)
            }
        }
    }
}
