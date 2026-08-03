use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

fn xbin() -> Command {
    let mut cmd = Command::cargo_bin("xbin").unwrap();
    cmd.env("NO_COLOR", "1");
    cmd
}

#[test]
fn test_help_output() {
    xbin()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("x.bin compiles"));
}

#[test]
fn test_version_output() {
    xbin()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("xbin"));
}

#[test]
fn test_completion_bash() {
    xbin()
        .args(["completion", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("complete"));
}

#[test]
fn test_completion_zsh() {
    xbin()
        .args(["completion", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("compdef"));
}

#[test]
fn test_completion_fish() {
    xbin()
        .args(["completion", "fish"])
        .assert()
        .success()
        .stdout(predicate::str::contains("complete"));
}

#[test]
fn test_man_output() {
    let dir = tempdir().unwrap();
    let man_dir = dir.path().join("man");
    xbin()
        .args(["man", man_dir.to_str().unwrap()])
        .assert()
        .success();
    assert!(man_dir.join("xbin.1").exists(), "xbin.1 should be created");
}

#[test]
fn test_doctor_output() {
    let output = xbin().arg("doctor").output().unwrap();
    assert!(
        output.status.code() == Some(0) || output.status.code() == Some(1),
        "doctor should exit 0 or 1"
    );
}

#[test]
fn test_doctor_strict() {
    let output = xbin().args(["doctor", "--strict"]).output().unwrap();
    assert!(
        output.status.code() == Some(0) || output.status.code() == Some(1),
        "doctor --strict should exit 0 or 1"
    );
}

#[test]
fn test_env_output() {
    xbin().arg("env").assert().success();
}

#[test]
fn test_clean_help() {
    xbin().args(["clean", "--help"]).assert().success();
}

#[test]
fn test_build_help_lists_sisr_flags() {
    xbin()
        .args(["build", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--enable-sisr"))
        .stdout(predicate::str::contains("--update-url"));
}

#[test]
fn test_build_dry_run_shows_sisr_when_enabled() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("app")).unwrap();
    std::fs::write(dir.path().join("app/package.json"), "{\"name\":\"app\"}").unwrap();
    xbin()
        .args([
            "build",
            dir.path().join("app").to_str().unwrap(),
            "--enable-sisr",
            "--update-url",
            "https://updates.example.com/app",
            "--dry-run",
            "--no-install",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("SISR:      enabled"))
        .stderr(predicate::str::contains(
            "Update URL: https://updates.example.com/app",
        ));
}

#[test]
fn test_build_dry_run_does_not_enable_sisr_by_default() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("app")).unwrap();
    std::fs::write(dir.path().join("app/package.json"), "{\"name\":\"app\"}").unwrap();
    xbin()
        .args([
            "build",
            dir.path().join("app").to_str().unwrap(),
            "--dry-run",
            "--no-install",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("SISR:      enabled").not());
}
