//! Chaos: recipient-machine trust. Empty trust store must reject; foreign
//! key must reject; publisher key after `trust` must accept.
//!
//! Uses a tiny fake stub so these stay in the sub-second range (no
//! `cargo build -p daedalus-stub` per test).

#![cfg(unix)]

use assert_cmd::Command;
use daedalus_core::assembly::{assemble_daedalus, AssemblyInput};
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use std::path::{Path, PathBuf};

fn daedalus() -> Command {
    let mut cmd = Command::cargo_bin("daedalus").unwrap();
    cmd.env("NO_COLOR", "1");
    cmd
}

fn write_keypair(dir: &Path) -> (PathBuf, PathBuf) {
    std::fs::create_dir_all(dir).unwrap();
    let sk = SigningKey::generate(&mut OsRng);
    let fp = hex::encode(sk.verifying_key().as_bytes());
    let key = dir.join(format!("{fp}.key"));
    let pubk = dir.join(format!("{fp}.pub"));
    std::fs::write(&key, sk.to_bytes()).unwrap();
    std::fs::write(&pubk, sk.verifying_key().as_bytes()).unwrap();
    (key, pubk)
}

fn assemble_signed(dir: &Path, key: &Path) -> PathBuf {
    let rootfs = dir.join("rootfs");
    std::fs::create_dir_all(rootfs.join("app")).unwrap();
    std::fs::write(rootfs.join("app/app.py"), b"print('chaos-trust-ok')\n").unwrap();
    let payload = daedalus_core::tar::create_tar_zstd_with_level(&rootfs, 1).unwrap();
    let meta = serde_json::json!({
        "name": "chaos-trust",
        "runtime": "python",
        "entrypoint": ["python3", "/app/app.py"],
    });
    let out = dir.join("app.de");
    assemble_daedalus(
        &out,
        &AssemblyInput {
            stub_bytes: b"FAKE-STUB-CHAOS-TRUST",
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
    out
}

/// One fixture, three courier assertions — avoids repeating keygen/sign/build.
#[test]
fn chaos_courier_trust_matrix() {
    let td = tempfile::tempdir().unwrap();
    let (publisher_key, publisher_pub) = write_keypair(&td.path().join("publisher"));
    let (_, foreign_pub) = write_keypair(&td.path().join("foreign"));
    let bin = assemble_signed(td.path(), &publisher_key);

    let empty = td.path().join("empty-trust");
    std::fs::create_dir_all(&empty).unwrap();
    daedalus()
        .args(["verify", bin.to_str().unwrap()])
        .env("DAEDALUS_TRUSTED_DIR", &empty)
        .assert()
        .failure()
        .stderr(predicates::str::contains("no trusted keys"));

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

    let recipient = td.path().join("recipient-trust");
    std::fs::create_dir_all(&recipient).unwrap();
    daedalus()
        .args([
            "trust",
            publisher_pub.to_str().unwrap(),
            "--trusted-dir",
            recipient.to_str().unwrap(),
        ])
        .assert()
        .success();
    daedalus()
        .args(["verify", bin.to_str().unwrap()])
        .env("DAEDALUS_TRUSTED_DIR", &recipient)
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
