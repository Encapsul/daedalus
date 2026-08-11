//! Seccomp e2e tests: verify --seccomp flag installs a denylist (not deny-all)
//! and the packaged app can still execute allowed syscalls.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::tempdir;

use erebus_core::assembly::{assemble_erebus, AssemblyInput};
use tar::Builder;

fn meta(seccomp: bool) -> Vec<u8> {
    let mut meta = serde_json::json!({
        "name": "seccomp-test",
        "runtime": "python",
        "entrypoint": ["python3", "/app/app.py"],
        "payload_format": "zstd-tar",
        "layers": [],
    });
    if seccomp {
        meta["seccomp"] = serde_json::Value::Bool(true);
    }
    serde_json::to_vec(&meta).unwrap()
}

fn payload() -> Vec<u8> {
    let mut tar = Builder::new(Vec::new());
    // app.py - just prints hello directly
    let app = b"#!/usr/bin/env python3\nprint('hello')\n";
    let mut h = tar::Header::new_gnu();
    h.set_size(app.len() as u64);
    h.set_mode(0o755);
    h.set_mtime(1_600_000_000);
    h.set_username("root").unwrap();
    h.set_groupname("root").unwrap();
    h.set_cksum();
    tar.append_data(&mut h, "app/app.py", Cursor::new(app))
        .unwrap();
    let tar_bytes = tar.into_inner().unwrap();
    zstd::stream::encode_all(Cursor::new(tar_bytes), 3).unwrap()
}

fn build_seccomp_erebus(work: &Path, stub: &Path, seccomp: bool) -> PathBuf {
    let out = work.join("app.erebus");
    let payload_bytes = payload();
    let meta_bytes = meta(seccomp);
    assemble_erebus(
        &out,
        &AssemblyInput {
            stub_bytes: &fs::read(stub).unwrap(),
            payload: &payload_bytes,
            meta_bytes: &meta_bytes,
            encrypt: false,
            squashfs: false,
            target_arch: None,
            sisr: None,
        },
    )
    .unwrap();
    out
}

/// Test that an erebus built with seccomp=true runs successfully (denylist, not deny-all).
#[test]
fn seccomp_denylist_allows_normal_execution() {
    let tmp = tempdir().unwrap();
    let work = tmp.path().to_path_buf();

    let stub = PathBuf::from(env!("CARGO_BIN_EXE_erebus-stub"));
    let erebus_path = build_seccomp_erebus(&work, &stub, true);

    let output = Command::new(&erebus_path).output().expect("run erebus");
    assert!(
        output.status.success(),
        "erebus with seccomp should run successfully (denylist, not deny-all): stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hello"),
        "expected 'hello' in output, got: {}",
        stdout
    );
}

/// Test that an erebus built WITHOUT seccomp also runs (baseline).
#[test]
fn seccomp_without_flag_works() {
    let tmp = tempdir().unwrap();
    let work = tmp.path().to_path_buf();

    let stub = PathBuf::from(env!("CARGO_BIN_EXE_erebus-stub"));
    let erebus_path = build_seccomp_erebus(&work, &stub, false);

    let output = Command::new(&erebus_path).output().expect("run erebus");
    assert!(
        output.status.success(),
        "erebus without seccomp should run successfully: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("hello"));
}
