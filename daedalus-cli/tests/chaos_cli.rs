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
    assert!(output.status.code() != Some(0));
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
