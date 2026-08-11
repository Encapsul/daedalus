//! Backup / restore / discard primitives for post-update rollback.
//!
//! The mission invariant ("the user is never left with a broken binary")
//! depends on the `.erebus.bak` snapshot being trustworthy at every instant:
//! `create_backup` snapshots the current binary before a SISR swap, the
//! snapshot is kept only until the health gate confirms the new version, and
//! `restore_backup` swaps it back over a failed new version. Every write goes
//! through [`AtomicWriter`], so an interrupted backup or restore can never
//! corrupt the live binary or the snapshot itself.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::sisr::swap::AtomicWriter;

/// Suffix appended to a binary's file name for its rollback snapshot.
pub const BACKUP_SUFFIX: &str = ".bak";

/// `/dir/app.erebus` → `/dir/app.erebus.bak`.
///
/// The snapshot lives next to the binary so the restore rename is atomic
/// (same filesystem) and so a renamed binary keeps a co-located snapshot.
pub fn backup_path_for(bin: &Path) -> PathBuf {
    let parent = bin.parent().unwrap_or_else(|| Path::new(""));
    let file = bin.file_name().map_or_else(
        || "app.erebus".to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    parent.join(format!("{file}{BACKUP_SUFFIX}"))
}

/// Atomically snapshots `src` (the running binary) to `backup`.
///
/// Both the bytes and the permission bits are copied; the snapshot is written
/// through a temporary file and renamed, so a crash mid-copy leaves no
/// partial snapshot at `backup`.
pub fn create_backup(src: &Path, backup: &Path) -> io::Result<()> {
    let parent = backup.parent().unwrap_or_else(|| Path::new(""));
    let mut src_file = fs::File::open(src)?;
    let mut writer = AtomicWriter::new(parent, "erebus.bak")?;
    io::copy(&mut src_file, writer.file_mut())?;
    let perms = fs::metadata(src)?.permissions();
    writer.file_mut().set_permissions(perms)?;
    writer.commit(backup)
}

/// Atomically swaps `backup` over `bin` — the rollback itself.
///
/// `bin` keeps its own path; only its bytes and permissions are replaced,
/// exactly like the forward SISR swap, so the on-disk path remains stable for
/// any open handles. Errors when the snapshot is missing.
pub fn restore_backup(bin: &Path, backup: &Path) -> io::Result<()> {
    if !backup.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no rollback snapshot at {}", backup.display()),
        ));
    }
    let parent = bin.parent().unwrap_or_else(|| Path::new(""));
    let mut src_file = fs::File::open(backup)?;
    let mut writer = AtomicWriter::new(parent, "erebus.restore")?;
    io::copy(&mut src_file, writer.file_mut())?;
    let perms = fs::metadata(backup)?.permissions();
    writer.file_mut().set_permissions(perms)?;
    writer.commit(bin)
}

/// Removes a snapshot once the new version is confirmed healthy.
///
/// Idempotent: a missing snapshot is not an error, so an interrupted
/// confirm-then-discard sequence cannot wedge future launches.
pub fn discard_backup(backup: &Path) -> io::Result<()> {
    match fs::remove_file(backup) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(unix)]
    fn bin_and_backup(tmp: &Path) -> (PathBuf, PathBuf) {
        let bin = tmp.join("app.erebus");
        fs::write(&bin, b"v1-bytes").unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
        (bin.clone(), backup_path_for(&bin))
    }

    #[test]
    fn backup_path_is_co_located() {
        let bin = Path::new("/home/u/app.erebus");
        assert_eq!(backup_path_for(bin), PathBuf::from("/home/u/app.erebus.bak"));
        let nested = Path::new("/opt/tools/bin/tool");
        assert_eq!(
            backup_path_for(nested),
            PathBuf::from("/opt/tools/bin/tool.bak")
        );
    }

    #[test]
    fn backup_path_handles_extensionless_binary() {
        let bin = Path::new("/srv/runner");
        assert_eq!(backup_path_for(bin), PathBuf::from("/srv/runner.bak"));
    }

    #[cfg(unix)]
    #[test]
    fn create_backup_snapshots_bytes_and_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let (bin, bak) = bin_and_backup(tmp.path());

        create_backup(&bin, &bak).unwrap();
        assert_eq!(fs::read(&bak).unwrap(), b"v1-bytes");
        assert_ne!(fs::metadata(&bak).unwrap().permissions().mode() & 0o111, 0);

        // The original is untouched and the backup is complete, not temp.
        assert_eq!(fs::read(&bin).unwrap(), b"v1-bytes");
        assert!(fs::read_dir(tmp.path()).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".tmp-")));
    }

    #[cfg(unix)]
    #[test]
    fn restore_backup_swaps_bytes_back_atomically() {
        let tmp = tempfile::tempdir().unwrap();
        let (bin, bak) = bin_and_backup(tmp.path());
        create_backup(&bin, &bak).unwrap();

        fs::write(&bin, b"v2-broken").unwrap();
        restore_backup(&bin, &bak).unwrap();
        assert_eq!(fs::read(&bin).unwrap(), b"v1-bytes");
    }

    #[cfg(unix)]
    #[test]
    fn restore_backup_errors_when_snapshot_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let (bin, bak) = bin_and_backup(tmp.path());
        let err = restore_backup(&bin, &bak).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert_eq!(fs::read(&bin).unwrap(), b"v1-bytes");
    }

    #[cfg(unix)]
    #[test]
    fn discard_backup_removes_and_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let (bin, bak) = bin_and_backup(tmp.path());
        create_backup(&bin, &bak).unwrap();

        discard_backup(&bak).unwrap();
        assert!(!bak.exists());
        discard_backup(&bak).unwrap();
    }
}
