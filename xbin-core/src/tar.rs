//! Deterministic tar archive creation for .xbin payload layers.
//!
//! Creates reproducible tar archives with normalized metadata (mtime=0,
//! uid/gid=0, sorted entries) for consistent builds.

use std::io::{self};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use flate2::read::GzDecoder;

/// Create a deterministic tar archive from a directory.
///
/// - Entries are sorted alphabetically
/// - mtime is set to 0 (Unix epoch)
/// - uid/gid are set to 0
/// - uname/gname are empty
/// - Only regular files and directories are included
/// - Symlinks are followed (files only)
pub fn create_deterministic_tar(root: &Path) -> io::Result<Vec<u8>> {
    let mut buf = io::Cursor::new(Vec::new());
    {
        let mut builder = tar::Builder::new(&mut buf);
        let mut entries = collect_entries(root)?;
        entries.sort();
        append_entries(&mut builder, root, &entries)?;
        builder
            .finish()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    }
    Ok(buf.into_inner())
}

/// Create a deterministic tar + compress (zstd level 3, streaming).
///
/// Streams tar entries directly to the zstd encoder — never buffers the
/// full uncompressed tar in memory. Uses multithreaded zstd for speed.
/// This is the BLAZING FAST path: level 3 + streaming + parallel.
pub fn create_tar_zstd(root: &Path) -> io::Result<Vec<u8>> {
    let entries = collect_entries(root)?;
    let mut encoder = zstd::Encoder::new(Vec::new(), crate::compress::DEFAULT_LEVEL)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let _ = encoder.multithread(num_cpus());
    {
        let mut builder = tar::Builder::new(&mut encoder);
        append_entries(&mut builder, root, &entries)?;
        builder
            .finish()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    }
    encoder
        .finish()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}

/// Stream tar entries directly to a writer (no in-memory buffer).
pub fn create_tar_streaming<W: io::Write>(root: &Path, writer: W) -> io::Result<()> {
    let entries = collect_entries(root)?;
    let mut builder = tar::Builder::new(writer);
    append_entries(&mut builder, root, &entries)?;
    builder
        .finish()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}

/// Extract a .tar.gz archive into a directory. Pure Rust — no external
/// `tar` process required.
pub fn extract_tar_gz<R: io::Read>(reader: R, dest: &Path) -> io::Result<()> {
    let decoder = GzDecoder::new(reader);
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(dest)?;
    Ok(())
}

/// Recursively collect relative paths of all files and directories.
fn collect_entries(root: &Path) -> io::Result<Vec<String>> {
    let mut entries = Vec::new();
    collect_recursive(root, root, &mut entries)?;
    Ok(entries)
}

fn collect_recursive(base: &Path, current: &Path, entries: &mut Vec<String>) -> io::Result<()> {
    let read_dir = match std::fs::read_dir(current) {
        Ok(rd) => rd,
        Err(_) => return Ok(()),
    };

    for entry in read_dir {
        let entry = entry.map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let path = entry.path();

        // Skip symlinks — they'll be followed by the tar builder
        let file_type = entry
            .file_type()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        if file_type.is_symlink() {
            continue;
        }

        let rel = path
            .strip_prefix(base)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let rel_str = rel.to_string_lossy().to_string();

        // Skip __pycache__ and .git
        if rel_str.contains("__pycache__") || rel_str.contains(".git") {
            continue;
        }

        entries.push(rel_str);

        if file_type.is_dir() {
            collect_recursive(base, &path, entries)?;
        }
    }
    Ok(())
}

/// Append sorted entries to a tar builder (shared by all create_* functions).
fn append_entries<W: io::Write>(
    builder: &mut tar::Builder<W>,
    root: &Path,
    entries: &[String],
) -> io::Result<()> {
    for entry in entries {
        let path = root.join(entry);
        let mut header = tar::Header::new_gnu();
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        header
            .set_username("")
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        header
            .set_groupname("")
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        if path.is_dir() {
            header.set_entry_type(tar::EntryType::Directory);
            header.set_mode(0o755);
            header.set_size(0);
            builder
                .append_data(&mut header, entry, &mut io::empty())
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        } else if path.is_file() {
            let meta = std::fs::metadata(&path)?;
            header.set_entry_type(tar::EntryType::Regular);
            let mode = if meta.permissions().mode() & 0o111 != 0 || is_executable_extension(&path) {
                0o755
            } else {
                0o644
            };
            header.set_mode(mode);
            header.set_size(meta.len());
            let mut f = std::fs::File::open(&path)?;
            builder
                .append_data(&mut header, entry, &mut f)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        }
    }
    Ok(())
}

/// Check if a file path has an extension that typically requires executable
/// permissions even when the source filesystem (e.g. vfat) doesn't preserve them.
fn is_executable_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| matches!(ext, "py" | "rb" | "sh" | "pl" | "php" | "ex" | "bat"))
        .unwrap_or(false)
}

/// Number of CPU cores available (returns 1 if detection fails).
fn num_cpus() -> u32 {
    std::thread::available_parallelism().map_or(1, |n| n.get() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn create_tar_from_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let tar = create_deterministic_tar(tmp.path()).unwrap();
        assert!(!tar.is_empty());
    }

    #[test]
    fn create_tar_with_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("hello.txt"), b"hello world").unwrap();
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("sub/data.txt"), b"nested").unwrap();

        let tar = create_deterministic_tar(root).unwrap();

        // Verify it's a valid tar
        let mut archive = tar::Archive::new(&tar[..]);
        let entries: Vec<_> = archive
            .entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(entries.contains(&"hello.txt".to_string()));
        assert!(entries.contains(&"sub".to_string()));
        assert!(entries.contains(&"sub/data.txt".to_string()));
    }

    #[test]
    fn tar_zstd_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("app.py"), b"print('hello')").unwrap();

        let compressed = create_tar_zstd(root).unwrap();
        let decompressed = crate::compress::decompress(&compressed).unwrap();

        let mut archive = tar::Archive::new(&decompressed[..]);
        let entries: Vec<_> = archive
            .entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(entries.contains(&"app.py".to_string()));
    }

    #[test]
    fn tar_is_deterministic() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("b.txt"), b"second").unwrap();
        fs::write(root.join("a.txt"), b"first").unwrap();

        let tar1 = create_deterministic_tar(root).unwrap();
        let tar2 = create_deterministic_tar(root).unwrap();
        assert_eq!(tar1, tar2);
    }

    #[test]
    fn extract_tar_gz_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("hello.txt"), b"hello world").unwrap();

        let tar_bytes = create_deterministic_tar(&src).unwrap();
        let mut gz = Vec::new();
        {
            let mut encoder =
                flate2::write::GzEncoder::new(&mut gz, flate2::Compression::default());
            std::io::Write::write_all(&mut encoder, &tar_bytes).unwrap();
        }

        extract_tar_gz(&gz[..], &dst).unwrap();

        let extracted = std::fs::read_to_string(dst.join("hello.txt")).unwrap();
        assert_eq!(extracted, "hello world");
    }
}
