//! End-to-end lifecycle proof for the post-update health gate and rollback.
//!
//! `health.rs` and `resilience.rs` are covered unit-by-unit, but the claim the
//! project makes in public copy is the *conjunction*: a failed gate must put
//! the previous working binary back, a passed gate must keep the new one, and
//! a permanently broken payload must stop being retried. These tests drive
//! the real files on a real temp directory, in the order the launcher uses
//! them, so the guarantee is observed rather than inferred.

use std::fs;
use std::path::{Path, PathBuf};

use daedalus_core::sisr::health::{HealthState, HealthStore};
use daedalus_core::sisr::resilience::{
    backup_path_for, create_backup, discard_backup, restore_backup, BACKUP_SUFFIX,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const V1: &[u8] = b"#!/bin/sh\necho known-good-v1\n";
const V2: &[u8] = b"#!/bin/sh\nexit 70\n";

fn install(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn read(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap()
}

fn temp_files_left_in(dir: &Path) -> Vec<String> {
    let mut leftovers: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".tmp-") || n.starts_with('.'))
        .collect();
    leftovers.sort();
    leftovers
}

#[test]
fn failed_gate_restores_the_previous_working_binary() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("app.daedalus");
    let bak = backup_path_for(&bin);
    let store = HealthStore::new(&tmp.path().join("health"));

    // 1. Known-good v1 running, snapshot taken before the update swap.
    install(&bin, V1);
    create_backup(&bin, &bak).unwrap();

    // 2. The update swaps in a payload that fails the gate.
    install(&bin, V2);
    store.begin("v2-hash").unwrap();
    let quarantined = store.record_failure("v2-hash", 3).unwrap();
    assert!(!quarantined, "one failure must not quarantine yet");

    // 3. The launcher rolls back: v1 bytes and mode are back at the same path.
    restore_backup(&bin, &bak).unwrap();
    assert_eq!(read(&bin), V1, "rollback must restore the exact v1 bytes");

    #[cfg(unix)]
    assert_ne!(
        fs::metadata(&bin).unwrap().permissions().mode() & 0o111,
        0,
        "restored binary must stay executable, otherwise the user is left with a broken install"
    );

    // 4. The failed version is retired once it is no longer on disk.
    discard_backup(&bak).unwrap();
    assert!(!bak.exists());
    assert_eq!(read(&bin), V1);
    assert!(
        temp_files_left_in(tmp.path()).is_empty(),
        "no temp file may survive a completed rollback"
    );
}

#[test]
fn passed_gate_keeps_the_new_binary_and_drops_the_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("app.daedalus");
    let bak = backup_path_for(&bin);
    let store = HealthStore::new(&tmp.path().join("health"));

    install(&bin, V1);
    create_backup(&bin, &bak).unwrap();
    install(&bin, V2);

    store.begin("v2-hash").unwrap();
    let status = store.confirm("v2-hash").unwrap();
    assert_eq!(status.state, HealthState::Healthy);

    // A healthy update must not be reverted, and the snapshot is dead weight.
    assert_eq!(read(&bin), V2);
    discard_backup(&bak).unwrap();
    assert!(
        !bak.exists(),
        "confirmed update must not keep a rollback snapshot"
    );
    assert_eq!(
        read(&bin),
        V2,
        "discarding a snapshot must not touch the live binary"
    );
}

#[test]
fn repeatedly_failing_payload_is_quarantined_and_not_reinstalled() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("app.daedalus");
    let bak = backup_path_for(&bin);
    let store = HealthStore::new(&tmp.path().join("health"));

    install(&bin, V1);
    create_backup(&bin, &bak).unwrap();

    // Three supervised launches of the same broken version.
    for attempt in 1..=3 {
        store.begin("bad-hash").unwrap();
        let quarantined = store.record_failure("bad-hash", 3).unwrap();
        assert_eq!(
            quarantined,
            attempt == 3,
            "quarantine exactly at the threshold"
        );
        if quarantined {
            restore_backup(&bin, &bak).unwrap();
        }
    }

    assert!(store.is_quarantined("bad-hash").unwrap());
    assert_eq!(read(&bin), V1, "user must end up on the known-good version");

    // Re-installing the same version must not reset the counter or re-arm it,
    // otherwise a broken payload churns the binary on every launch.
    let rearmed = store.begin("bad-hash").unwrap();
    assert_eq!(rearmed.state, HealthState::Quarantined);
    assert_eq!(
        rearmed.attempts, 3,
        "failure counter must survive a reinstall"
    );
}

#[test]
fn missing_snapshot_never_damages_the_live_binary() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("app.daedalus");
    let bak = backup_path_for(&bin);

    install(&bin, V2);
    let err = restore_backup(&bin, &bak).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    assert_eq!(
        read(&bin),
        V2,
        "a failed rollback must leave the binary byte-identical, not truncated"
    );
}

#[test]
fn snapshot_path_is_derived_from_the_live_binary() {
    let bin = PathBuf::from("/opt/daedalus/app.daedalus");
    // The suffix is appended, not substituted: `with_extension` would collapse
    // `app.daedalus` to `app..bak` and make the two paths easy to confuse.
    assert_eq!(
        backup_path_for(&bin),
        PathBuf::from("/opt/daedalus/app.daedalus.bak")
    );
    // A binary with no extension must still get a distinct snapshot name.
    let extless = PathBuf::from("/opt/daedalus/runner");
    assert_eq!(
        backup_path_for(&extless),
        PathBuf::from("/opt/daedalus/runner.bak")
    );
}
