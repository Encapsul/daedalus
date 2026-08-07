//! Atomic file replacement primitives for SISR updates.
//!
//! The guarantee that matters (mission invariant): if the reconstruction is
//! interrupted at any point, the current binary must never be altered or left
//! half-written. We achieve this by writing to a temporary file in the same
//! directory, fsyncing it, then atomically renaming it over the destination.
//! [`AtomicWriter`] deletes the temporary on drop unless committed, so both
//! error returns and process exits leave the destination untouched.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// A writable temporary file that replaces `dst` atomically on [`commit`].
///
/// Until `commit` succeeds the destination is never touched. If the writer is
/// dropped uncommitted (error return, early `?`, process death), the
/// temporary is removed — a stale `.tmp` can only survive a hard kill, and is
/// never mistaken for the binary.
///
/// [`commit`]: Self::commit
pub struct AtomicWriter {
    file: File,
    path: PathBuf,
    committed: bool,
}

impl AtomicWriter {
    /// Creates an empty temporary file inside `dir` with a `tag`ged name.
    pub fn new(dir: &Path, tag: &str) -> io::Result<Self> {
        let path = dir.join(format!(".{tag}.tmp-{}", std::process::id()));
        let file = File::create(&path)?;
        Ok(Self {
            file,
            path,
            committed: false,
        })
    }

    /// Path of the temporary file (for diagnostics only).
    pub fn temp_path(&self) -> &Path {
        &self.path
    }

    /// Mutable handle to the underlying file for streaming writes.
    pub fn file_mut(&mut self) -> &mut File {
        &mut self.file
    }

    /// Flushes, fsyncs, then atomically renames over `dst`.
    ///
    /// On POSIX this is `rename(2)` — atomic even if `dst` exists. On Windows
    /// `std::fs::rename` maps to `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING`
    /// (a locked destination requires the trampoline fallback, out of scope
    /// here — the launcher is Linux-only).
    pub fn commit(mut self, dst: &Path) -> io::Result<()> {
        self.file.flush()?;
        self.file.sync_all()?;
        fs::rename(&self.path, dst)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for AtomicWriter {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Atomically replaces `dst` with `src`.
///
/// `src` must already be fully written and synced. This is the final step of
/// a two-phase update: build the new binary to a `.tmp` file (see
/// [`AtomicWriter`]), then `atomic_replace` swaps it in one syscall.
pub fn atomic_replace(src: &Path, dst: &Path) -> io::Result<()> {
    fs::rename(src, dst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_replaces_destination_and_cleans_tmp() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let dst = dir.join("app.xbin");
        fs::write(&dst, b"old").unwrap();

        let mut w = AtomicWriter::new(dir, "app.xbin.sisr").unwrap();
        let tmp_path = w.temp_path().to_path_buf();
        assert!(tmp_path.exists());
        w.file_mut().write_all(b"new-content").unwrap();
        w.commit(&dst).unwrap();

        assert_eq!(fs::read(&dst).unwrap(), b"new-content");
        assert!(!tmp_path.exists(), "tmp must be removed after commit");
    }

    #[test]
    fn drop_without_commit_leaves_destination_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let dst = dir.join("app.xbin");
        fs::write(&dst, b"original").unwrap();

        {
            let mut w = AtomicWriter::new(dir, "app.xbin.sisr").unwrap();
            let tmp_path = w.temp_path().to_path_buf();
            w.file_mut().write_all(b"partial").unwrap();
            // Simulated interruption: drop without commit.
            drop(w);
            assert!(!tmp_path.exists(), "tmp must be removed on drop");
        }

        assert_eq!(
            fs::read(&dst).unwrap(),
            b"original",
            "destination must never be altered by an interrupted write"
        );
    }

    #[test]
    fn commit_error_leaves_destination_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let dst = dir.join("app.xbin");
        fs::write(&dst, b"original").unwrap();

        // Commit to a path inside a missing directory -> rename fails.
        let mut w = AtomicWriter::new(dir, "app.xbin.sisr").unwrap();
        w.file_mut().write_all(b"new").unwrap();
        let missing = dir.join("nope").join("app.xbin");
        assert!(w.commit(&missing).is_err());
        assert_eq!(fs::read(&dst).unwrap(), b"original");
    }

    #[test]
    fn atomic_replace_overwrites_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("new.xbin");
        let dst = tmp.path().join("app.xbin");
        fs::write(&src, b"v2").unwrap();
        fs::write(&dst, b"v1").unwrap();

        atomic_replace(&src, &dst).unwrap();
        assert_eq!(fs::read(&dst).unwrap(), b"v2");
        assert!(!src.exists());
    }

    #[test]
    fn atomic_replace_missing_source_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let dst = tmp.path().join("app.xbin");
        fs::write(&dst, b"v1").unwrap();
        assert!(atomic_replace(&tmp.path().join("missing"), &dst).is_err());
        assert_eq!(fs::read(&dst).unwrap(), b"v1");
    }
}
