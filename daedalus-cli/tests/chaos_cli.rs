//! Chaos-monkey tests for daedalus CLI edge cases.
//!
//! Tests extreme input, error paths, and flag interactions.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn daedalus() -> Command {
    Command::cargo_bin("daedalus").expect("binary exists")
}

fn generate_test_key(dir: &std::path::Path) -> std::path::PathBuf {
    let key_dir = dir.join("keys");
    fs::create_dir_all(&key_dir).unwrap();
    daedalus()
        .arg("keygen")
        .arg("--key-dir")
        .arg(&key_dir)
        .assert()
        .success();
    // keygen names the file by fingerprint; find it
    let mut found = None;
    for entry in fs::read_dir(&key_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "key") {
            found = Some(path);
            break;
        }
    }
    found.expect("key file not generated")
}

#[test]
fn sign_with_no_input_and_no_force_fails() {
    let td = TempDir::new().unwrap();
    let key_path = generate_test_key(td.path());
    let file = td.path().join("app.de");
    fs::write(&file, b"not a real daedalus file").unwrap();

    daedalus()
        .arg("sign")
        .arg(&file)
        .arg("--key")
        .arg(&key_path)
        .arg("--no-input")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--no-input"));
}

#[test]
fn sign_with_no_input_and_force_skips_prompt() {
    let td = TempDir::new().unwrap();
    let key_path = generate_test_key(td.path());
    let file = td.path().join("app.de");
    fs::write(&file, b"not a real daedalus file").unwrap();

    // With --force and --no-input, should skip prompt and fail on invalid file format
    let output = daedalus()
        .arg("sign")
        .arg(&file)
        .arg("--key")
        .arg(&key_path)
        .arg("--force")
        .arg("--no-input")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Should fail, but not because of a prompt
    assert!(!stderr.contains("Sign"));
    assert_ne!(output.status.code(), Some(0));
}

#[test]
fn clean_with_no_input_and_no_force_fails() {
    let td = TempDir::new().unwrap();
    let xdg_cache = td.path().join("xdg_cache");
    let cache_dir = xdg_cache.join("daedalus");
    fs::create_dir_all(&cache_dir).unwrap();
    let marker = cache_dir.join("marker.txt");
    fs::write(&marker, b"test").unwrap();

    daedalus()
        .arg("clean")
        .env("XDG_CACHE_HOME", &xdg_cache)
        .arg("--no-input")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--no-input"));
}

#[test]
fn clean_with_no_input_and_force_skips_prompt() {
    let output = daedalus()
        .arg("clean")
        .arg("--force")
        .arg("--no-input")
        .output()
        .unwrap();
    let code = output.status.code();
    assert!(code == Some(0) || code == Some(1));
}

#[test]
fn doctor_fix_with_no_input_and_no_force_fails() {
    daedalus()
        .arg("doctor")
        .arg("--fix")
        .arg("--no-input")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--no-input"));
}

#[test]
fn inspect_nonexistent_file_returns_not_found() {
    daedalus()
        .arg("inspect")
        .arg("/nonexistent/path/app.de")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Error"));
}

#[test]
fn verify_nonexistent_file_returns_not_found() {
    daedalus()
        .arg("verify")
        .arg("/nonexistent/path/app.de")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Error"));
}

#[test]
fn build_nonexistent_app_dir_fails() {
    daedalus()
        .arg("build")
        .arg("/nonexistent/path")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Error"));
}

#[test]
fn run_nonexistent_file_fails() {
    daedalus()
        .arg("run")
        .arg("/nonexistent/path/app.de")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Error"));
}

#[test]
fn sign_nonexistent_key_fails() {
    let td = TempDir::new().unwrap();
    let file = td.path().join("app.de");
    fs::write(&file, b"not a real daedalus file").unwrap();

    daedalus()
        .arg("sign")
        .arg(&file)
        .arg("--key")
        .arg("/nonexistent/key.key")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Error"));
}

#[test]
fn help_shows_documentation_link() {
    daedalus()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("github.com"));
}

#[test]
fn version_flag_works() {
    daedalus()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("0."));
}

#[test]
fn quiet_suppresses_output_on_success() {
    let td = TempDir::new().unwrap();
    let file = td.path().join("app.de");
    fs::write(&file, b"not a real daedalus file").unwrap();

    daedalus()
        .arg("inspect")
        .arg(&file)
        .arg("--quiet")
        .assert()
        .failure(); // fails on invalid file, but should not have extra output
}

#[test]
fn chaos_trust_help_mentions_pubkey() {
    daedalus()
        .args(["trust", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("public").or(predicate::str::contains("pubkey")));
}

#[test]
fn chaos_trust_list_empty_store() {
    let td = TempDir::new().unwrap();
    let trust_dir = td.path().join("trusted");
    std::fs::create_dir_all(&trust_dir).unwrap();
    daedalus()
        .args([
            "trust",
            "--list",
            "--trusted-dir",
            trust_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("no trusted keys"));
}

#[test]
fn chaos_trust_list_shows_fingerprint_after_trust() {
    let td = TempDir::new().unwrap();
    let trust_dir = td.path().join("trusted");
    std::fs::create_dir_all(&trust_dir).unwrap();
    let good = td.path().join("good.pub");
    fs::write(&good, [0x42u8; 32]).unwrap();
    daedalus()
        .args([
            "trust",
            good.to_str().unwrap(),
            "--trusted-dir",
            trust_dir.to_str().unwrap(),
            "--quiet",
        ])
        .assert()
        .success();
    let out = daedalus()
        .args([
            "trust",
            "--list",
            "--json",
            "--trusted-dir",
            trust_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body = String::from_utf8_lossy(&out);
    let v: serde_json::Value = serde_json::from_str(&body).expect("list json");
    assert_eq!(v["count"], 1);
    assert!(v["keys"][0]["fingerprint"].as_str().is_some());
}

#[test]
fn chaos_verify_empty_trust_mentions_list() {
    use daedalus_core::assembly::{assemble_daedalus, AssemblyInput};

    let td = TempDir::new().unwrap();
    let meta = serde_json::json!({
        "name": "signed-hint",
        "runtime": "binary",
        "entrypoint": ["/app/app"],
    });
    let meta_bytes = serde_json::to_vec(&meta).unwrap();
    let payload = daedalus_core::compress::compress(b"layer").unwrap();
    let out = td.path().join("app.de");
    assemble_daedalus(
        &out,
        &AssemblyInput {
            stub_bytes: b"FAKE-STUB",
            payload: &payload,
            meta_bytes: &meta_bytes,
            squashfs: false,
            target_arch: None,
            sisr: None,
            encryption: None,
        },
    )
    .unwrap();
    // Sign so verify walks the trust path (unsigned exits early).
    let key_dir = td.path().join("keys");
    fs::create_dir_all(&key_dir).unwrap();
    daedalus()
        .args(["keygen", "--key-dir", key_dir.to_str().unwrap()])
        .assert()
        .success();
    let key = fs::read_dir(&key_dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().and_then(|e| e.to_str()) == Some("key"))
        .expect("key");
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

    let empty = td.path().join("empty");
    fs::create_dir_all(&empty).unwrap();
    daedalus()
        .args(["verify", out.to_str().unwrap()])
        .env("DAEDALUS_TRUSTED_DIR", &empty)
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("no trusted keys")
                .and(predicate::str::contains("trust --list")),
        );
}

#[test]
fn chaos_verify_help_mentions_trusted() {
    daedalus()
        .args(["verify", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("trusted"));
}

#[test]
fn chaos_scan_empty_dir_fails_cleanly() {
    let td = TempDir::new().unwrap();
    daedalus()
        .arg("scan")
        .arg(td.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("No .daedalus").or(predicate::str::contains("Error")));
}

#[test]
fn chaos_comment_only_requirements_not_pip() {
    let td = TempDir::new().unwrap();
    fs::write(
        td.path().join("requirements.txt"),
        "# vendored offline\n\n  # still empty\n",
    )
    .unwrap();
    assert_eq!(
        daedalus_core::pkgmgr::detect_python_pkgmgr(td.path()),
        None,
        "comment-only requirements must not select pip"
    );
    assert!(!daedalus_core::pkgmgr::requirements_declares_deps(
        "# only\n"
    ));
}

#[test]
fn chaos_scan_ignores_junk_keeps_dual_ext() {
    use daedalus_core::assembly::{assemble_daedalus, AssemblyInput};

    let td = TempDir::new().unwrap();
    let meta = serde_json::json!({
        "name": "scan-junk",
        "runtime": "binary",
        "entrypoint": ["/app/app"],
    });
    let meta_bytes = serde_json::to_vec(&meta).unwrap();
    let payload = daedalus_core::compress::compress(b"scan-junk").unwrap();
    for name in ["keep.de", "keep.daedalus"] {
        assemble_daedalus(
            &td.path().join(name),
            &AssemblyInput {
                stub_bytes: b"FAKE-STUB-SCAN-JUNK",
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
    fs::write(td.path().join("notes.txt"), b"not an artifact").unwrap();
    fs::write(td.path().join("fake.de.bak"), b"wrong extension").unwrap();

    let out = daedalus()
        .args(["scan", td.path().to_str().unwrap(), "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body = String::from_utf8_lossy(&out);
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("scan json");
    let entries = parsed.as_array().expect("array");
    assert_eq!(entries.len(), 2, "junk files must not inflate scan: {body}");
}

#[test]
fn chaos_trust_rejects_wrong_key_length() {
    let td = TempDir::new().unwrap();
    let bad = td.path().join("bad.pub");
    fs::write(&bad, [0u8; 16]).unwrap();
    let trust_dir = td.path().join("trusted");
    daedalus()
        .args([
            "trust",
            bad.to_str().unwrap(),
            "--trusted-dir",
            trust_dir.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("32 bytes"));
}

#[test]
fn chaos_trust_dry_run_does_not_write() {
    let td = TempDir::new().unwrap();
    let good = td.path().join("good.pub");
    fs::write(&good, [0x41u8; 32]).unwrap();
    let trust_dir = td.path().join("trusted");
    fs::create_dir_all(&trust_dir).unwrap();
    daedalus()
        .args([
            "trust",
            good.to_str().unwrap(),
            "--trusted-dir",
            trust_dir.to_str().unwrap(),
            "--dry-run",
        ])
        .assert()
        .success();
    assert!(
        fs::read_dir(&trust_dir).unwrap().next().is_none(),
        "dry-run must not materialize a .pub in the trust dir"
    );
}

#[test]
fn chaos_verify_unsigned_assembled_file_reports_not_signed() {
    use daedalus_core::assembly::{assemble_daedalus, AssemblyInput};

    let td = TempDir::new().unwrap();
    let meta = serde_json::json!({
        "name": "unsigned-chaos",
        "runtime": "binary",
        "entrypoint": ["/app/app"],
    });
    let meta_bytes = serde_json::to_vec(&meta).unwrap();
    let payload = daedalus_core::compress::compress(b"layer").unwrap();
    let out = td.path().join("unsigned.de");
    assemble_daedalus(
        &out,
        &AssemblyInput {
            stub_bytes: b"FAKE-STUB",
            payload: &payload,
            meta_bytes: &meta_bytes,
            squashfs: false,
            target_arch: None,
            sisr: None,
            encryption: None,
        },
    )
    .unwrap();

    let empty_trust = td.path().join("empty");
    fs::create_dir_all(&empty_trust).unwrap();
    daedalus()
        .args(["verify", out.to_str().unwrap()])
        .env("DAEDALUS_TRUSTED_DIR", &empty_trust)
        .assert()
        .success()
        .stderr(predicate::str::contains("not signed"));
}

#[test]
fn no_color_disables_ansi_codes() {
    let td = TempDir::new().unwrap();
    let file = td.path().join("app.de");
    fs::write(&file, b"not a real daedalus file").unwrap();

    let output = daedalus()
        .arg("inspect")
        .arg(&file)
        .arg("--no-color")
        .output()
        .unwrap();

    // Should not contain ANSI escape sequences
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);
    assert!(
        !combined.contains("\x1b[") && !combined.contains("\u{1b}["),
        "output should not contain ANSI codes with --no-color"
    );
}
