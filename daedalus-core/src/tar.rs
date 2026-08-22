//! Deterministic tar archive creation for .daedalus payload layers.
//!
//! Creates reproducible tar archives with normalized metadata (mtime=0,
//! uid/gid=0, sorted entries) for consistent builds.

use std::io::{self};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;

/// Create a deterministic tar archive from a directory.
///
/// - Entries are sorted alphabetically
/// - mtime is set to 0 (Unix epoch)
/// - uid/gid are set to 0
/// - uname/gname are empty
/// - Symlinks are stored as symlink entries with targets guarded to stay
///   inside the root (see [`guarded_symlink_target`])
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
/// full uncompressed tar in memory. Single-threaded zstd keeps the output
/// byte-identical across machines so the payload hash is reproducible.
pub fn create_tar_zstd(root: &Path) -> io::Result<Vec<u8>> {
    create_tar_zstd_with_level(root, crate::compress::DEFAULT_LEVEL)
}

/// Same as [`create_tar_zstd`] but with a caller-chosen zstd compression level.
///
/// Level 1 = fastest decompression / largest artifact; level 3 = default
/// balance; level 19 = smallest artifact / slowest build. The stub's
/// single-threaded decompressor benefits most from lower levels when cold
/// start time matters more than on-disk footprint.
pub fn create_tar_zstd_with_level(root: &Path, level: i32) -> io::Result<Vec<u8>> {
    let entries = collect_entries(root)?;
    let mut encoder = zstd::Encoder::new(Vec::new(), level)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
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

        let rel = path
            .strip_prefix(base)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let rel_str = rel.to_string_lossy().to_string();

        // Exclude version-control and bytecode-cache trees by exact component
        // name so files like `.gitignore` / `.gitattributes` stay packaged.
        if rel.components().any(|c| {
            matches!(
                c,
                Component::Normal(n) if n == ".git" || n == "__pycache__"
            )
        }) {
            continue;
        }

        let file_type = entry
            .file_type()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        entries.push(rel_str);

        // Symlinked dirs are stored as links, never recursed into (cycle risk).
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

        let meta = std::fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            if let Some(target) = guarded_symlink_target(root, &path) {
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_mode(0o777);
                header.set_size(0);
                builder
                    .append_link(&mut header, entry, &target)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            }
        } else if path.is_dir() {
            header.set_entry_type(tar::EntryType::Directory);
            header.set_mode(0o755);
            header.set_size(0);
            builder
                .append_data(&mut header, entry, &mut io::empty())
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        } else {
            let mode = if is_executable_file(&path) {
                0o755
            } else {
                0o644
            };
            header.set_entry_type(tar::EntryType::Regular);
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

/// Resolve a symlink target lexically and return it only if it stays within
/// `root`. Absolute or root-escaping links are dropped: inside the packaged
/// rootfs they would be broken, or reach outside the sandbox at runtime.
fn guarded_symlink_target(root: &Path, link: &Path) -> Option<String> {
    let target = std::fs::read_link(link).ok()?;
    if target.is_absolute() {
        return None;
    }
    let resolved = lexically_normalize(&link.parent()?.join(&target));
    resolved
        .starts_with(root)
        .then(|| target.to_string_lossy().into_owned())
}

/// Collapse `.` and `..` components without touching the filesystem.
fn lexically_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Check if a file path has an extension that typically requires executable
/// permissions even when the source filesystem (e.g. vfat) doesn't preserve them.
fn is_executable_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| matches!(ext, "py" | "rb" | "sh" | "pl" | "php" | "ex" | "bat"))
        .unwrap_or(false)
}

/// Whether a file should be marked executable in the tar entry. On unix the
/// source permission bits decide; on other platforms (e.g. vfat or windows)
/// only the extension-based heuristic applies.
fn is_executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        let mode = std::fs::metadata(path)
            .map(|m| m.permissions().mode())
            .unwrap_or(0);
        if mode & 0o111 != 0 {
            return true;
        }
    }
    is_executable_extension(path)
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
    fn symlink_preserved_as_symlink_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("real.js"), b"module").unwrap();
        std::os::unix::fs::symlink("real.js", root.join("alias.js")).unwrap();

        let tar = create_deterministic_tar(root).unwrap();
        let mut archive = tar::Archive::new(&tar[..]);
        let entry = archive
            .entries()
            .unwrap()
            .find(|e| {
                e.as_ref()
                    .map(|e| e.path().unwrap().to_string_lossy() == "alias.js")
                    .unwrap_or(false)
            })
            .unwrap()
            .unwrap();
        assert!(entry.header().entry_type().is_symlink());
        assert_eq!(
            entry.link_name().unwrap().unwrap().to_string_lossy(),
            "real.js"
        );
    }

    #[test]
    fn escaping_symlink_target_is_dropped() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("real.txt"), b"data").unwrap();
        // Escapes the packaged root via `..`
        std::os::unix::fs::symlink("../../etc/passwd", root.join("evil.txt")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", root.join("abs.txt")).unwrap();

        let tar = create_deterministic_tar(root).unwrap();
        let mut archive = tar::Archive::new(&tar[..]);
        let paths: Vec<_> = archive
            .entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(paths.contains(&"real.txt".to_string()));
        assert!(!paths.contains(&"evil.txt".to_string()));
        assert!(!paths.contains(&"abs.txt".to_string()));
    }

    #[test]
    fn git_dir_excluded_but_gitignore_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".git/config"), b"repo").unwrap();
        fs::write(root.join(".gitignore"), b"*.log").unwrap();
        fs::write(root.join(".gitattributes"), b"* text=auto").unwrap();
        fs::create_dir(root.join("__pycache__")).unwrap();
        fs::write(root.join("__pycache__/mod.cpython-311.pyc"), b"x").unwrap();

        let tar = create_deterministic_tar(root).unwrap();
        let mut archive = tar::Archive::new(&tar[..]);
        let paths: Vec<_> = archive
            .entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(paths.contains(&".gitignore".to_string()));
        assert!(paths.contains(&".gitattributes".to_string()));
        assert!(!paths.contains(&".git".to_string()));
        assert!(!paths.contains(&".git/config".to_string()));
        assert!(!paths.contains(&"__pycache__".to_string()));
    }

    #[test]
    fn tar_zstd_is_byte_deterministic() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("app.py"), b"print('hello')").unwrap();
        fs::create_dir(root.join("static")).unwrap();
        fs::write(root.join("static/site.css"), b"body{}").unwrap();

        assert_eq!(
            create_tar_zstd(root).unwrap(),
            create_tar_zstd(root).unwrap()
        );
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
