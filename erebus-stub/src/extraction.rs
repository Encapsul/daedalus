//! Atomic extraction for the erebus launcher stub.
//!
//! Provides `extract_atomic` (zstd+tar) and `extract_squashfs_atomic`
//! (squashfs), both built on the shared `atomic_extract` helper.
//! All operations are atomic: extraction happens in a tmp directory and
//! is renamed into place only on success.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

use crate::nanos;

/// Default maximum total decompressed bytes across all tar entries (1 GB).
const DEFAULT_MAX_DECOMPRESSED_BYTES: u64 = 1024 * 1024 * 1024;
/// Default maximum number of files in a single tar archive.
const DEFAULT_MAX_FILES: usize = 50_000;

/// Decompression-bomb limits for `extract_atomic`. Both are configurable at
/// runtime via env vars so operators can tighten (or relax) them per workload
/// without rebuilding the stub:
/// - `XBIN_MAX_EXTRACT_SIZE`  — total decompressed bytes (default 1 GiB)
/// - `XBIN_MAX_EXTRACT_FILES` — entry/file count (default 50,000)
///
/// The env vars are read once per extraction and only from the launcher's own
/// process environment — they are NOT parsed from the (untrusted) payload, so
/// an attacker cannot weaken the limits by smuggling values into the erebus.
struct ExtractLimits {
    max_bytes: u64,
    max_files: usize,
}

impl ExtractLimits {
    fn from_env() -> Self {
        let max_bytes = std::env::var("XBIN_MAX_EXTRACT_SIZE")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(DEFAULT_MAX_DECOMPRESSED_BYTES);
        let max_files = std::env::var("XBIN_MAX_EXTRACT_FILES")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(DEFAULT_MAX_FILES);
        Self {
            max_bytes,
            max_files,
        }
    }
}

/// Whether a cached extraction can be trusted as-is.
///
/// The warm path skips signature/integrity verification and execs whatever
/// lives at `cache_root/rootfs`; that is only sound if the cache directory is
/// owned by the current user and not group/other-writable (a wrong owner or a
/// 0o777 dir means someone else could have planted the rootfs). On non-Unix
/// platforms there is no permission model to inspect, so we trust the cache.
#[cfg(unix)]
pub fn cache_root_trustworthy(cache_root: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let Ok(meta) = fs::metadata(cache_root) else {
        return false;
    };
    // SAFETY: geteuid(2) cannot fail and has no invalid inputs; the effective
    // uid identifies who the kernel would credit as owner of a file we create.
    let euid = unsafe { libc::geteuid() };
    if !meta.is_dir() || meta.uid() != euid {
        return false;
    }
    // Group/other-write bits would let any co-resident user replace the
    // extracted rootfs with their own code, so treat such a cache as absent.
    meta.mode() & 0o022 == 0
}

/// Non-Unix platforms have no permission model to inspect — trust the cache.
#[cfg(not(unix))]
pub fn cache_root_trustworthy(_cache_root: &Path) -> bool {
    true
}

/// Extract zstd-compressed tar blobs atomically into `cache_root/rootfs`.
pub fn extract_atomic(blobs: &[&[u8]], cache_root: &Path) -> io::Result<()> {
    atomic_extract(cache_root, |tmp_rootfs| {
        let limits = ExtractLimits::from_env();
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
                if size > limits.max_bytes {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("tar entry exceeds max size: {size} > {}", limits.max_bytes),
                    ));
                }
                total_bytes = total_bytes.saturating_add(size);
                if total_bytes > limits.max_bytes {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "total decompressed size {total_bytes} exceeds {}",
                            limits.max_bytes
                        ),
                    ));
                }
                file_count = file_count.saturating_add(1);
                if file_count > limits.max_files {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "file count exceeds max: {file_count} > {}",
                            limits.max_files
                        ),
                    ));
                }
                entry.unpack_in(tmp_rootfs)?;
            }
        }
        Ok(())
    })
}

/// Extract squashfs blobs atomically into `cache_root/rootfs`.
pub fn extract_squashfs_atomic(blobs: &[&[u8]], cache_root: &Path) -> io::Result<()> {
    atomic_extract(cache_root, |tmp_rootfs| {
        crate::squashfs_extract::extract_squashfs_layers(blobs, tmp_rootfs)
    })
}

/// Shared atomic extraction: create tmp dir, run extraction closure, write .ready, rename.
///
/// On rename failure, a stale `cache_root` (no `.ready`) is wiped and the
/// rename is retried once; a `cache_root` that carries a valid `.ready` is a
/// previous successful extraction and is used as-is.
pub fn atomic_extract(
    cache_root: &Path,
    extract_fn: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<()> {
    let parent = cache_root.parent().unwrap_or(Path::new("/tmp"));
    fs::create_dir_all(parent)?;

    let tmp = parent.join(format!(".tmp-{}-{}", std::process::id(), nanos()));
    let tmp_rootfs = tmp.join("rootfs");
    fs::create_dir_all(&tmp_rootfs)?;

    extract_fn(&tmp_rootfs)?;

    // A freshly extracted cache is private to the invoking user regardless of
    // umask; otherwise a lax umask would produce a group/other-writable cache
    // that `cache_root_trustworthy` then rejects on every warm start.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o700))?;
    }

    File::create(tmp.join(".ready"))?.write_all(b"1")?;

    // The marker lives at cache_root/.ready (the tmp dir becomes cache_root
    // on rename), so the stale-partial check below must look there.
    let marker = cache_root.join(".ready");
    match fs::rename(&tmp, cache_root) {
        Ok(()) => Ok(()),
        Err(e) => {
            // A valid .ready marker means a concurrent/successful extraction
            // already completed — surface the error but don't block the run.
            // The marker only legitimizes the cache when the directory is
            // owned by us with sane perms; a foreign cache is wiped instead.
            if marker.exists() && cache_root_trustworthy(cache_root) {
                let _ = fs::remove_dir_all(&tmp);
                eprintln!("[erebus] warning: cache rename failed but existing cache is valid: {e}");
                Ok(())
            } else {
                // Otherwise cache_root is a stale partial extraction (no
                // .ready). Wipe it and retry the rename once with tmp intact,
                // instead of leaving the binary permanently stuck on every
                // cold start.
                let _ = fs::remove_dir_all(cache_root);
                match fs::rename(&tmp, cache_root) {
                    Ok(()) => Ok(()),
                    Err(e2) => {
                        let _ = fs::remove_dir_all(&tmp);
                        Err(e2)
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_app(tmp_rootfs: &Path) -> io::Result<()> {
        fs::create_dir_all(tmp_rootfs.join("app"))?;
        fs::write(tmp_rootfs.join("app/app.py"), b"print('hi')")
    }

    #[test]
    fn extract_creates_marker_and_rootfs() {
        let dir = TempDir::new().unwrap();
        let cache_root = dir.path().join("hash");

        atomic_extract(&cache_root, write_app).unwrap();

        assert!(cache_root.join(".ready").is_file());
        assert!(cache_root.join("rootfs/app/app.py").is_file());
    }

    #[test]
    fn stale_cache_root_is_wiped_and_retried() {
        let dir = TempDir::new().unwrap();
        let cache_root = dir.path().join("hash");
        fs::create_dir_all(cache_root.join("rootfs")).unwrap();
        fs::write(cache_root.join("rootfs/partial"), b"leftover").unwrap();

        atomic_extract(&cache_root, write_app).unwrap();

        assert!(cache_root.join(".ready").is_file());
        assert!(cache_root.join("rootfs/app/app.py").is_file());
        assert!(!cache_root.join("rootfs/partial").exists());
    }

    #[test]
    #[cfg(unix)]
    fn valid_marker_survives_rename_failure() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        let cache_root = dir.path().join("hash");
        fs::create_dir_all(cache_root.join("rootfs")).unwrap();
        fs::write(cache_root.join("rootfs/old"), b"previous").unwrap();
        fs::write(cache_root.join(".ready"), b"1").unwrap();
        // The marker only legitimizes a cache the current user owns with sane
        // perms; pin 0o755 so the test does not depend on the shell's umask.
        fs::set_permissions(&cache_root, fs::Permissions::from_mode(0o755)).unwrap();

        atomic_extract(&cache_root, write_app).unwrap();

        assert!(cache_root.join("rootfs/old").exists());
    }

    #[cfg(unix)]
    #[test]
    fn trustworthy_cache_requires_private_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        let cache_root = dir.path().join("hash");
        fs::create_dir_all(&cache_root).unwrap();

        fs::set_permissions(&cache_root, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            cache_root_trustworthy(&cache_root),
            "a user-owned, group/other-read-only dir is trustworthy"
        );

        fs::set_permissions(&cache_root, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(
            !cache_root_trustworthy(&cache_root),
            "a group/other-writable dir must be rejected"
        );

        assert!(!cache_root_trustworthy(&dir.path().join("absent")));
    }
}
