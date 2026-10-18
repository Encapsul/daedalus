//! Chaos: recipient-machine trust. Empty trust store must reject; foreign
//! key must reject; publisher key after `trust` must accept.
//!
//! This is the USB / courier threat model encoded as tests.

#![cfg(unix)]

use assert_cmd::Command;
use daedalus_core::assembly::{assemble_daedalus, AssemblyInput};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

mod common;
use common::locate_stub;

fn daedalus() -> Command {
    let mut cmd = Command::cargo_bin("daedalus").unwrap();
    cmd.env("NO_COLOR", "1");
    cmd
}

fn assemble_signed_fixture(dir: &Path, stub: &Path, key: &Path) -> PathBuf {
    let rootfs = dir.join("rootfs");
    std::fs::create_dir_all(rootfs.join("app")).unwrap();
    std::fs::write(rootfs.join("app/app.py"), b"print('chaos-trust-ok')\n").unwrap();
    let payload = daedalus_core::tar::create_tar_zstd_with_level(&rootfs, 3).unwrap();
    let meta = serde_json::json!({
        "name": "chaos-trust",
        "runtime": "python",
        "entrypoint": ["python3", "/app/app.py"],
    });
    let out = dir.join("app.de");
    let stub_bytes = std::fs::read(stub).unwrap();
    assemble_daedalus(
        &out,
        &AssemblyInput {
            stub_bytes: &stub_bytes,
            payload: &payload,
            meta_bytes: serde_json::to_vec(&meta).unwrap().as_slice(),
            squashfs: false,
            target_arch: None,
            sisr: None,
            encryption: None,
        },
    )
    .unwrap();

    daedalus()
        .args([
            "sign",
            out.to_str().unwrap(),
            "--key",
            key.to_str().unwrap(),
            "--force",
            "--no-input",
        ])
        .assert()
        .success();
    std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755)).unwrap();
    out
}

fn keygen(dir: &Path) -> (PathBuf, PathBuf) {
    let key_dir = dir.join("keys");
    std::fs::create_dir_all(&key_dir).unwrap();
    daedalus()
        .args(["keygen", "--key-dir", key_dir.to_str().unwrap()])
        .assert()
        .success();
    let mut key = None;
    let mut pubk = None;
    for entry in std::fs::read_dir(&key_dir).unwrap() {
        let p = entry.unwrap().path();
        match p.extension().and_then(|e| e.to_str()) {
            Some("key") => key = Some(p),
            Some("pub") => pubk = Some(p),
            _ => {}
        }
    }
    (key.expect("key"), pubk.expect("pub"))
}

#[test]
fn chaos_verify_fails_with_empty_trust_store() {
    let Some(stub) = locate_stub() else {
        eprintln!("skipping: no stub");
        return;
    };
    let td = tempfile::tempdir().unwrap();
    let (key, _pub) = keygen(td.path());
    let bin = assemble_signed_fixture(td.path(), &stub, &key);

    let empty_trust = td.path().join("empty-trust");
    std::fs::create_dir_all(&empty_trust).unwrap();

    daedalus()
        .args(["verify", bin.to_str().unwrap()])
        .env("DAEDALUS_TRUSTED_DIR", &empty_trust)
        .assert()
        .failure()
        .stderr(predicates::str::contains("no trusted keys"));
}

#[test]
fn chaos_verify_fails_under_foreign_key() {
    let Some(stub) = locate_stub() else {
        eprintln!("skipping: no stub");
        return;
    };
    let td = tempfile::tempdir().unwrap();
    let (publisher_key, _) = keygen(&td.path().join("publisher"));
    let (_, foreign_pub) = keygen(&td.path().join("foreign"));
    let bin = assemble_signed_fixture(td.path(), &stub, &publisher_key);

    let foreign_trust = td.path().join("foreign-trust");
    std::fs::create_dir_all(&foreign_trust).unwrap();
    daedalus()
        .args([
            "trust",
            foreign_pub.to_str().unwrap(),
            "--trusted-dir",
            foreign_trust.to_str().unwrap(),
        ])
        .assert()
        .success();

    daedalus()
        .args(["verify", bin.to_str().unwrap()])
        .env("DAEDALUS_TRUSTED_DIR", &foreign_trust)
        .assert()
        .failure();
}

#[test]
fn chaos_verify_ok_after_recipient_trusts_publisher() {
    let Some(stub) = locate_stub() else {
        eprintln!("skipping: no stub");
        return;
    };
    let td = tempfile::tempdir().unwrap();
    let (publisher_key, publisher_pub) = keygen(&td.path().join("publisher"));
    let bin = assemble_signed_fixture(td.path(), &stub, &publisher_key);

    let recipient_trust = td.path().join("recipient-trust");
    std::fs::create_dir_all(&recipient_trust).unwrap();
    daedalus()
        .args([
            "trust",
            publisher_pub.to_str().unwrap(),
            "--trusted-dir",
            recipient_trust.to_str().unwrap(),
        ])
        .assert()
        .success();

    daedalus()
        .args(["verify", bin.to_str().unwrap()])
        .env("DAEDALUS_TRUSTED_DIR", &recipient_trust)
        .assert()
        .success();
}

#[test]
fn chaos_scan_finds_both_extensions() {
    let td = tempfile::tempdir().unwrap();
    let meta = serde_json::json!({
        "name": "scan-chaos",
        "runtime": "binary",
        "entrypoint": ["/app/app"],
    });
    let meta_bytes = serde_json::to_vec(&meta).unwrap();
    let payload = daedalus_core::compress::compress(b"scan-layer").unwrap();
    let stub = b"FAKE-STUB-FOR-SCAN";
    for name in ["one.de", "two.daedalus"] {
        let out = td.path().join(name);
        assemble_daedalus(
            &out,
            &AssemblyInput {
                stub_bytes: stub,
                payload: &payload,
                meta_bytes: &meta_bytes,
                squashfs: false,
                target_arch: None,
                sisr: None,
                encryption: None,
            },
        )
        .unwrap();
    }

    let out = daedalus()
        .args(["scan", td.path().to_str().unwrap(), "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body = String::from_utf8_lossy(&out);
    let parsed: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|e| panic!("scan JSON: {e}: {body}"));
    let entries = parsed
        .as_array()
        .unwrap_or_else(|| panic!("scan JSON must be an array: {body}"));
    assert_eq!(
        entries.len(),
        2,
        "scan must find both .de and .daedalus: {body}"
    );
    assert!(
        entries
            .iter()
            .all(|e| e.get("name").and_then(|v| v.as_str()) == Some("scan-chaos")),
        "both artifacts must inspect: {body}"
    );
}
