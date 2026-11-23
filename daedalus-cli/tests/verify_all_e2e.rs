//! `daedalus verify --all`: one at-rest report covering the signature scheme,
//! stub coverage, integrity hash, metadata and the experimental gate.

use assert_cmd::Command;
use daedalus_core::assembly::{assemble_daedalus, AssemblyInput};
use predicates::prelude::*;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

fn daedalus() -> Command {
    let mut cmd = Command::cargo_bin("daedalus").unwrap();
    cmd.env("NO_COLOR", "1");
    cmd
}

/// A valid unsigned `.de` with a fake stub (verify only reads the footer).
fn fake_de(dir: &Path) -> PathBuf {
    let payload = daedalus_core::compress::compress(b"not-a-real-layer").unwrap();
    let meta = serde_json::json!({
        "name": "verify-all",
        "version": "1.2.3",
        "runtime": "binary",
        "entrypoint": ["/app/app"],
        "isolation": 0,
        "layers": [],
    });
    let out = dir.join("app.de");
    let stub = b"FAKE-STUB".to_vec();
    assemble_daedalus(
        &out,
        &AssemblyInput {
            stub_bytes: &stub,
            payload: &payload,
            meta_bytes: &serde_json::to_vec(&meta).unwrap(),
            squashfs: false,
            target_arch: None,
            sisr: None,
            encryption: None,
        },
    )
    .unwrap();
    out
}

/// Generate a key pair; returns (private key, public key).
fn keygen(dir: &Path) -> (PathBuf, PathBuf) {
    let key_dir = dir.join("keys");
    daedalus()
        .args(["keygen", "--key-dir", key_dir.to_str().unwrap()])
        .assert()
        .success();
    let key = std::fs::read_dir(&key_dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "key"))
        .expect("private key");
    let pubkey = std::fs::read_dir(&key_dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "pub"))
        .expect("public key");
    (key, pubkey)
}

fn sign(de: &Path, key: &Path) {
    daedalus()
        .args([
            "sign",
            de.to_str().unwrap(),
            "--key",
            key.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .success();
}

fn trust(pubkey: &Path, trust_dir: &Path) {
    daedalus()
        .args([
            "trust",
            pubkey.to_str().unwrap(),
            "--trusted-dir",
            trust_dir.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn verify_all_reports_at_rest_bindings() {
    let dir = tempdir().unwrap();
    let de = fake_de(dir.path());
    let (key, pubkey) = keygen(dir.path());
    sign(&de, &key);
    let trust_dir = dir.path().join("trust");
    trust(&pubkey, &trust_dir);

    let out = daedalus()
        .args([
            "verify",
            de.to_str().unwrap(),
            "--all",
            "--json",
            "--trusted-dir",
            trust_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "verify --all failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["mode"], "all");
    assert_eq!(v["verified"], true);
    assert_eq!(v["scheme"], "trailing");
    assert_eq!(v["signed"], true);
    assert_eq!(v["stub_bound_signature"], true);
    assert_eq!(v["integrity_ok"], true);
    assert_eq!(v["metadata"]["name"], "verify-all");
    assert_eq!(v["metadata"]["version"], "1.2.3");
    assert!(Path::new(v["key"].as_str().unwrap())
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pub")));
}

#[test]
fn verify_all_rejects_a_tampered_payload() {
    let dir = tempdir().unwrap();
    let de = fake_de(dir.path());
    let (key, pubkey) = keygen(dir.path());
    sign(&de, &key);
    let trust_dir = dir.path().join("trust");
    trust(&pubkey, &trust_dir);

    let mut bytes = std::fs::read(&de).unwrap();
    let footer = daedalus_core::format::Footer::read_from(&mut Cursor::new(&bytes)).unwrap();
    let idx = footer.payload_offset as usize + 1;
    bytes[idx] ^= 0xFF;
    std::fs::write(&de, &bytes).unwrap();

    daedalus()
        .args([
            "verify",
            de.to_str().unwrap(),
            "--all",
            "--trusted-dir",
            trust_dir.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("integrity hash mismatch"));
}

#[test]
fn verify_all_rejects_an_unsigned_artifact() {
    let dir = tempdir().unwrap();
    let de = fake_de(dir.path());
    daedalus()
        .args(["verify", de.to_str().unwrap(), "--all", "--no-input"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not signed"));
}

#[test]
fn dashboard_is_experimental() {
    daedalus()
        .arg("dashboard")
        .assert()
        .failure()
        .stderr(predicate::str::contains("is experimental"))
        .stderr(predicate::str::contains("--experimental"));
}
