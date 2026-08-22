//! Seccomp e2e tests: verify --seccomp flag installs a denylist (not deny-all)
//! and the packaged app can still execute allowed syscalls.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::tempdir;

use daedalus_core::assembly::{assemble_daedalus, AssemblyInput};
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

fn build_seccomp_daedalus(work: &Path, stub: &Path, seccomp: bool) -> PathBuf {
    let out = work.join("app.daedalus");
    let payload_bytes = payload();
    let meta_bytes = meta(seccomp);
    assemble_daedalus(
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

/// Test that an daedalus built with seccomp=true runs successfully (denylist, not deny-all).
#[test]
fn seccomp_denylist_allows_normal_execution() {
    let tmp = tempdir().unwrap();
    let work = tmp.path().to_path_buf();

    let stub = PathBuf::from(env!("CARGO_BIN_EXE_daedalus-stub"));
    let daedalus_path = build_seccomp_daedalus(&work, &stub, true);

    let output = Command::new(&daedalus_path).output().expect("run daedalus");
    assert!(
        output.status.success(),
        "daedalus with seccomp should run successfully (denylist, not deny-all): stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hello"),
        "expected 'hello' in output, got: {}",
        stdout
    );
}

/// Test that an daedalus built WITHOUT seccomp also runs (baseline).
#[test]
fn seccomp_without_flag_works() {
    let tmp = tempdir().unwrap();
    let work = tmp.path().to_path_buf();

    let stub = PathBuf::from(env!("CARGO_BIN_EXE_daedalus-stub"));
    let daedalus_path = build_seccomp_daedalus(&work, &stub, false);

    let output = Command::new(&daedalus_path).output().expect("run daedalus");
    assert!(
        output.status.success(),
        "daedalus without seccomp should run successfully: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("hello"));
}

/// Phase 5b: Exec capability enforcement — a `RuntimeLayer` WITHOUT `exec`
/// capability must refuse the launch ("Exec capability absent").
fn build_layered_daedalus(work: &Path, stub: &Path, capabilities: &[&str]) -> PathBuf {
    let out = work.join("layered.daedalus");
    let caps_json: Vec<serde_json::Value> =
        capabilities.iter().map(|c| serde_json::json!(c)).collect();
    let meta = serde_json::json!({
        "name": "cap-test",
        "runtime": "python",
        "entrypoint": ["python3", "/app/app.py"],
        "payload_format": "zstd-tar",
        "layers": [
            {
                "kind": "runtime",
                "name": "python",
                "interpreter": "python3",
                "entrypoint": ["python3", "/app/app.py"],
                "version": "3.11",
                "env": [],
                "capabilities": caps_json,
            }
        ],
        "entrypoint_layer": "python",
    });
    let meta_bytes = serde_json::to_vec(&meta).unwrap();
    assemble_daedalus(
        &out,
        &AssemblyInput {
            stub_bytes: &fs::read(stub).unwrap(),
            payload: &payload(),
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

/// Phase 5b: When a `RuntimeLayer` has NO `exec` capability and seccomp is
/// NOT active, the stub refuses the launch outright (no fork needed).
#[test]
fn exec_capability_absent_refuses_launch() {
    let tmp = tempdir().unwrap();
    let work = tmp.path();
    let stub = PathBuf::from(env!("CARGO_BIN_EXE_daedalus-stub"));
    let daedalus_path = build_layered_daedalus(work, &stub, &["network"]);

    let output = Command::new(&daedalus_path).output().expect("run daedalus");
    assert!(
        !output.status.success(),
        "launch without Exec capability should fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Exec capability absent"),
        "expected 'Exec capability absent' in stderr, got: {}",
        stderr
    );
}

/// Phase 5b: When a `RuntimeLayer` HAS `exec` capability, the launch proceeds
/// (may fail later if python3 is missing, but the capability check passes).
#[test]
fn exec_capability_present_allows_check() {
    let tmp = tempdir().unwrap();
    let work = tmp.path();
    let stub = PathBuf::from(env!("CARGO_BIN_EXE_daedalus-stub"));
    let daedalus_path = build_layered_daedalus(work, &stub, &["exec"]);

    let output = Command::new(&daedalus_path).output().expect("run daedalus");
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Should NOT say "Exec capability absent" — it got past the check.
    assert!(
        !stderr.contains("Exec capability absent"),
        "Exec capability present should pass capability check, got: {}",
        stderr
    );
}
