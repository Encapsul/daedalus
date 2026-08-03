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

/// `upgrade-binary` turns a legacy (SISR-less) file into a valid v2 file and
/// preserves the original payload bytes.
#[test]
fn test_upgrade_binary_converts_legacy_file() {
    use std::io::Cursor;
    use xbin_core::assembly::assemble_xbin;
    use xbin_core::format::Footer;
    use xbin_core::sisr_header::read_sisr;

    let dir = tempdir().unwrap();
    let input = dir.path().join("legacy.xbin");
    assemble_xbin(
        &input,
        b"STUB_DATA",
        b"PAYLOAD_PAYLOAD_PAYLOAD_PAYLOAD",
        br#"{"name":"legacy"}"#,
        false,
        false,
        None,
    )
    .unwrap();

    let before = std::fs::read(&input).unwrap();
    let in_footer = Footer::read_from(&mut Cursor::new(&before)).unwrap();

    let output = dir.path().join("migrated.xbin");
    xbin()
        .args([
            "upgrade-binary",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Upgraded"))
        .stderr(predicate::str::contains("manifest written"));

    let after = std::fs::read(&output).unwrap();
    let out_footer = Footer::read_from(&mut Cursor::new(&after)).unwrap();
    assert_ne!(out_footer.flags & xbin_core::format::FLAG_SISR, 0);
    assert_eq!(out_footer.payload_sha256, in_footer.payload_sha256);

    let payload_end = (in_footer.payload_offset + in_footer.payload_csize) as usize;
    assert_eq!(&after[..payload_end], &before[..payload_end]);

    let (_, manifest) = read_sisr(&mut Cursor::new(&after))
        .unwrap()
        .expect("migrated file must embed a SISR manifest");
    assert_eq!(manifest.payload_len, in_footer.payload_csize);

    assert!(dir.path().join("migrated.xbin.manifest").exists());
}
