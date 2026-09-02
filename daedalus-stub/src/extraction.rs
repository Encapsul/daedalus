//! Atomic extraction for the daedalus launcher stub.
//!
//! Provides `extract_atomic` (zstd+tar) and `extract_squashfs_atomic`
//! (squashfs), both built on the shared `atomic_extract` helper.
//! All operations are atomic: extraction happens in a tmp directory and
//! is renamed into place only on success.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;

use crate::nanos;
use tar::Archive;
use zstd::Decoder;

/// Default maximum total decompressed bytes across all tar entries (1 GB).
const DEFAULT_MAX_DECOMPRESSED_BYTES: u64 = 1024 * 1024 * 1024;
/// Default maximum number of files in a single tar archive.
const DEFAULT_MAX_FILES: usize = 50_000;

/// Decompression-bomb limits for `extract_atomic`. Both are configurable at
/// runtime via env vars so operators can tighten (or relax) them per workload
/// without rebuilding the stub:
/// - `DAEDALUS_MAX_EXTRACT_SIZE`  — total decompressed bytes (default 1 GiB)
/// - `DAEDALUS_MAX_EXTRACT_FILES` — entry/file count (default 50,000)
///
/// The env vars are read once per extraction and only from the launcher's own
/// process environment — they are NOT parsed from the (untrusted) payload, so
/// an attacker cannot weaken the limits by smuggling values into the daedalus.
struct ExtractLimits {
    max_bytes: u64,
    max_files: usize,
}

impl ExtractLimits {
    /// `from_env` - read decompression limits from environment variables.
    ///
    /// Description:
    /// Reads DAEDALUS_MAX_EXTRACT_SIZE (bytes, default 1 GiB) and
    /// DAEDALUS_MAX_EXTRACT_FILES (default 50000) from the process environment.
    /// Values are capped at safe ceilings to prevent runaway configuration:
    /// - `DAEDALUS_MAX_EXTRACT_SIZE` is capped at 10 GiB.
    /// - `DAEDALUS_MAX_EXTRACT_FILES` is capped at 500 thousand.
    ///
    /// Return: the `Self`
    fn from_env() -> Self {
        let max_bytes = std::env::var("DAEDALUS_MAX_EXTRACT_SIZE")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|v| v.min(10 * 1024 * 1024 * 1024))
            .unwrap_or(DEFAULT_MAX_DECOMPRESSED_BYTES);
        let max_files = std::env::var("DAEDALUS_MAX_EXTRACT_FILES")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .map(|v| v.min(500_000))
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
/// `cache_root_trustworthy` - check whether the cache root is owned and private.
/// @cache_root: cache root
///
/// Description:
/// Returns true when the directory is owned by the current user and has no
/// group/other write bits.
///
/// Return: true or false
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
/// `cache_root_trustworthy` - trust the cache on non-Unix platforms.
/// @_cache_root: cache root
///
/// Description:
/// Always returns true on non-Unix platforms.
///
/// Return: true or false
pub fn cache_root_trustworthy(_cache_root: &Path) -> bool {
    true
}

/// Check whether the on-disk source binary still matches the cache.
///
/// Stores the source file's size + mtime in `<cache_root>/.source` (JSON).
/// On warm-start this detects byte-flips in the payload that would keep the
/// footer's SHA-256 cache key unchanged.
pub fn source_manifest_matches(
    cache_root: &Path,
    expected_size: u64,
    expected_mtime: std::time::SystemTime,
) -> bool {
    let manifest_path = cache_root.join(".source");
    let content = match std::fs::read_to_string(&manifest_path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let Ok(manifest) = serde_json::from_str::<SourceManifest>(&content) else {
        return false;
    };
    manifest.size == expected_size
        && manifest.mtime_ns
            == expected_mtime
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
}

#[derive(serde::Deserialize, serde::Serialize)]
struct SourceManifest {
    size: u64,
    mtime_ns: u128,
}

/// Record the source file's identity after a successful extraction so the
/// warm path can detect tampering.
pub fn write_source_manifest(
    cache_root: &Path,
    size: u64,
    mtime: std::time::SystemTime,
) -> io::Result<()> {
    let manifest = SourceManifest {
        size,
        mtime_ns: mtime
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    };
    let path = cache_root.join(".source");
    let json = serde_json::to_string(&manifest)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("serde_json: {e}")))?;
    std::fs::write(path, json)
}

/// Extract zstd-compressed tar blobs atomically into `cache_root/rootfs`.
///
/// Description:
/// Decompresses each blob with zstd, unpacks the tar archive, enforces
/// decompression-bomb limits, and rejects symlinks with absolute targets
/// or `..` traversal. Extraction is atomic via tmp+rename.
///
/// Return: nothing
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
                // Defense-in-depth: reject symlinks/hardlinks whose target
                // escapes the rootfs (absolute paths or `..` traversal).
                // A signed artifact shouldn't contain these, but a compromised
                // key or cache would bypass signature verification.
                if let Some(link_name) = entry.link_name()? {
                    let link_str = link_name.to_string_lossy();
                    if link_str.starts_with('/') || link_str.contains("..") {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("symlink/hardlink target escapes rootfs: {link_str}"),
                        ));
                    }
                }
                entry.unpack_in(tmp_rootfs)?;
            }
        }
        Ok(())
    })
}

/// Extract squashfs blobs atomically into `cache_root/rootfs`.
///
/// Description:
/// Delegates to squashfs_extract::extract_squashfs_layers inside the atomic
/// tmp+rename wrapper.
///
/// Return: nothing
pub fn extract_squashfs_atomic(blobs: &[&[u8]], cache_root: &Path) -> io::Result<()> {
    atomic_extract(cache_root, |tmp_rootfs| {
        crate::squashfs_extract::extract_squashfs_layers(blobs, tmp_rootfs)
    })
}

/// Extract zstd-compressed tar blobs lazily: priority files first, then
/// background-extract the rest.
///
/// Description:
/// 1. Create a tmp dir and extract only priority files into it.
/// 2. Write `.ready` and atomically rename tmp -> cache_root.
/// 3. Spawn a background thread to extract remaining files into the now-live
///    cache_root. The background thread writes `.lazy_done` when complete.
///
/// Return: nothing
pub fn extract_atomic_lazy(
    blobs: &[&[u8]],
    cache_root: &Path,
    priority_files: &[PathBuf],
) -> io::Result<()> {
    let parent = cache_root.parent().unwrap_or(Path::new("/tmp"));
    fs::create_dir_all(parent)?;

    let tmp = parent.join(format!(".tmp-{}-{}", std::process::id(), nanos()));
    let tmp_rootfs = tmp.join("rootfs");
    fs::create_dir_all(&tmp_rootfs)?;

    let mut decoder = Decoder::new(io::Cursor::new(blobs[0]))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("zstd: {e}")))?;
    let mut archive = Archive::new(&mut decoder);
    archive.set_preserve_permissions(true);
    archive.set_overwrite(true);

    let limits = ExtractLimits::from_env();
    let mut total_bytes: u64 = 0;
    let mut file_count: usize = 0;
    let mut priority_set: std::collections::HashSet<PathBuf> =
        priority_files.iter().cloned().collect();

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

        let path = entry.path()?;
        let path_buf = path.to_path_buf();

        let is_priority = priority_set.contains(&path_buf);
        if !is_priority {
            continue;
        }

        if let Some(link_name) = entry.link_name()? {
            let link_str = link_name.to_string_lossy();
            if link_str.starts_with('/') || link_str.contains("..") {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("symlink/hardlink target escapes rootfs: {link_str}"),
                ));
            }
        }

        entry.unpack_in(&tmp_rootfs)?;
        priority_set.remove(&path_buf);
    }

    if !priority_set.is_empty() {
        let missing: Vec<_> = priority_set.iter().map(|p| p.to_string_lossy()).collect();
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "priority files not found in payload: {}",
                missing.join(", ")
            ),
        ));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o700))?;
    }

    File::create(tmp.join(".ready"))?.write_all(b"1")?;

    let marker = cache_root.join(".ready");
    match fs::rename(&tmp, cache_root) {
        Ok(()) => {}
        Err(e) => {
            if marker.exists() && cache_root_trustworthy(cache_root) {
                let _ = fs::remove_dir_all(&tmp);
                eprintln!(
                    "[daedalus] warning: cache rename failed but existing cache is valid: {e}"
                );
                return Ok(());
            }
            let _ = fs::remove_dir_all(cache_root);
            fs::rename(&tmp, cache_root)?;
        }
    }

    let cache_root_bg = cache_root.to_path_buf();
    let blobs_bg: Vec<Vec<u8>> = blobs.iter().map(|b| b.to_vec()).collect();
    let _ = thread::spawn(move || {
        let _ = extract_remaining(&blobs_bg, &cache_root_bg);
    });

    Ok(())
}

/// Extract remaining (non-priority) files from the payload into an already
/// live cache_root.
fn extract_remaining(blobs: &[Vec<u8>], cache_root: &Path) -> io::Result<()> {
    let rootfs = cache_root.join("rootfs");
    let mut decoder = Decoder::new(io::Cursor::new(&blobs[0]))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("zstd: {e}")))?;
    let mut archive = Archive::new(&mut decoder);
    archive.set_preserve_permissions(true);
    archive.set_overwrite(true);

    let limits = ExtractLimits::from_env();
    let mut total_bytes: u64 = 0;
    let mut file_count: usize = 0;

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

        if let Some(link_name) = entry.link_name()? {
            let link_str = link_name.to_string_lossy();
            if link_str.starts_with('/') || link_str.contains("..") {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("symlink/hardlink target escapes rootfs: {link_str}"),
                ));
            }
        }

        if let Err(e) = entry.unpack_in(&rootfs) {
            eprintln!("[daedalus] lazy extract warning: {e}");
        }
    }

    let _ = File::create(cache_root.join(".lazy_done"));
    Ok(())
}

/// Shared atomic extraction: create tmp dir, run extraction closure, write .ready, rename.
///
/// On rename failure, a stale `cache_root` (no `.ready`) is wiped and the
/// rename is retried once; a `cache_root` that carries a valid `.ready` is a
/// previous successful extraction and is used as-is.
///
/// Description:
/// Runs the extraction closure in a tmp directory, sets 0o700 permissions,
/// writes the .ready marker, and atomically renames into cache_root.
///
/// Return: nothing
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
                eprintln!(
                    "[daedalus] warning: cache rename failed but existing cache is valid: {e}"
                );
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

    /// `write_app` - write app.
    /// `@tmp_rootfs`: tmp rootfs
    /// `@io`: io
    ///
    /// Description:
    ///
    /// Return: Result containing `io::Result<()>`
    fn write_app(tmp_rootfs: &Path) -> io::Result<()> {
        fs::create_dir_all(tmp_rootfs.join("app"))?;
        fs::write(tmp_rootfs.join("app/app.py"), b"print('hi')")
    }

    #[test]
    /// `extract_creates_marker_and_rootfs` - extract creates marker and rootfs.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn extract_creates_marker_and_rootfs() {
        let dir = TempDir::new().unwrap();
        let cache_root = dir.path().join("hash");

        atomic_extract(&cache_root, write_app).unwrap();

        assert!(cache_root.join(".ready").is_file());
        assert!(cache_root.join("rootfs/app/app.py").is_file());
    }

    #[test]
    /// `stale_cache_root_is_wiped_and_retried` - stale cache root is wiped and retried.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// `valid_marker_survives_rename_failure` - valid marker survives rename failure.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// `trustworthy_cache_requires_private_permissions` - trustworthy cache requires private permissions.
    ///
    /// Description:
    ///
    /// Return: nothing
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

    /// Chaos: tar symlinks with absolute targets or `..` traversal must be
    /// rejected to prevent path traversal outside the rootfs.
    #[cfg(unix)]
    /// `build_symlink_tar` - build symlink tar.
    /// `@link_target`: link target
    ///
    /// Description:
    ///
    /// Return: vector of Vec<u8>
    fn build_symlink_tar(link_target: &str) -> Vec<u8> {
        let mut tar = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_path("evil_link").unwrap();
        header.set_link_name(link_target).unwrap();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_cksum();
        tar.append(&header, &mut &[][..]).unwrap();
        tar.into_inner().unwrap()
    }

    /// `compress_zstd` - compress zstd.
    /// `@data`: data
    ///
    /// Description:
    ///
    /// Return: vector of Vec<u8>
    fn compress_zstd(data: &[u8]) -> Vec<u8> {
        let cursor = std::io::Cursor::new(data);
        zstd::stream::encode_all(cursor, 0).unwrap()
    }

    #[test]
    /// `extract_rejects_absolute_symlink` - extract rejects absolute symlink.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn extract_rejects_absolute_symlink() {
        let dir = TempDir::new().unwrap();
        let cache_root = dir.path().join("hash");
        let tar_data = build_symlink_tar("/etc/passwd");
        let compressed = compress_zstd(&tar_data);

        let result = extract_atomic(&[&compressed], &cache_root);
        assert!(result.is_err(), "absolute symlink must be rejected");
        let err = result.unwrap_err();
        eprintln!("DEBUG: actual error = {err}");
        assert!(err.to_string().contains("escapes rootfs"));
    }

    #[test]
    /// `extract_rejects_traversal_symlink` - extract rejects traversal symlink.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn extract_rejects_traversal_symlink() {
        let dir = TempDir::new().unwrap();
        let cache_root = dir.path().join("hash");
        let tar_data = build_symlink_tar("../../../etc/passwd");
        let compressed = compress_zstd(&tar_data);

        let result = extract_atomic(&[&compressed], &cache_root);
        assert!(result.is_err(), "traversal symlink must be rejected");
        assert!(result.unwrap_err().to_string().contains("escapes rootfs"));
    }

    #[test]
    /// `extract_allows_safe_relative_symlink` - extract allows safe relative symlink.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn extract_allows_safe_relative_symlink() {
        let dir = TempDir::new().unwrap();
        let cache_root = dir.path().join("hash");
        let tar_data = build_symlink_tar("app/data.txt");
        let compressed = compress_zstd(&tar_data);

        let result = extract_atomic(&[&compressed], &cache_root);
        assert!(
            result.is_ok(),
            "safe relative symlink should be allowed: {:?}",
            result.err()
        );
    }
}
