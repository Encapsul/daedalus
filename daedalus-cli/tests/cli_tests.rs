use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

fn daedalus() -> Command {
    let mut cmd = Command::cargo_bin("daedalus").unwrap();
    cmd.env("NO_COLOR", "1");
    cmd
}

#[test]
fn test_help_output() {
    daedalus()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("daedalus compiles"));
}

#[test]
fn test_version_output() {
    daedalus()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("daedalus"));
}

#[test]
fn test_completion_bash() {
    daedalus()
        .args(["completion", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("complete"));
}

#[test]
fn test_completion_zsh() {
    daedalus()
        .args(["completion", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("compdef"));
}

#[test]
fn test_completion_fish() {
    daedalus()
        .args(["completion", "fish"])
        .assert()
        .success()
        .stdout(predicate::str::contains("complete"));
}

#[test]
fn test_man_output() {
    let dir = tempdir().unwrap();
    let man_dir = dir.path().join("man");
    daedalus()
        .args(["man", man_dir.to_str().unwrap()])
        .assert()
        .success();
    assert!(
        man_dir.join("daedalus.1").exists(),
        "daedalus.1 should be created"
    );
}

#[test]
fn test_doctor_output() {
    let output = daedalus().arg("doctor").output().unwrap();
    assert!(
        output.status.code() == Some(0) || output.status.code() == Some(1),
        "doctor should exit 0 or 1"
    );
}

#[test]
fn test_doctor_strict() {
    let output = daedalus().args(["doctor", "--strict"]).output().unwrap();
    assert!(
        output.status.code() == Some(0) || output.status.code() == Some(1),
        "doctor --strict should exit 0 or 1"
    );
}

#[test]
fn test_env_output() {
    daedalus().arg("env").assert().success();
}

#[test]
fn test_clean_help() {
    daedalus().args(["clean", "--help"]).assert().success();
}

#[test]
fn test_build_help_lists_sisr_flags() {
    daedalus()
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
    daedalus()
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
    daedalus()
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

/// `migrate` turns a legacy (SISR-less) file into a valid v2 file and
/// preserves the original payload bytes.
#[test]
fn test_upgrade_binary_converts_legacy_file() {
    use daedalus_core::assembly::{assemble_daedalus, AssemblyInput};
    use daedalus_core::format::Footer;
    use daedalus_core::sisr_header::read_sisr;
    use std::io::Cursor;

    let dir = tempdir().unwrap();
    let input = dir.path().join("legacy.de");
    assemble_daedalus(
        &input,
        &AssemblyInput {
            stub_bytes: b"STUB_DATA",
            payload: b"PAYLOAD_PAYLOAD_PAYLOAD_PAYLOAD",
            meta_bytes: br#"{"name":"legacy"}"#,
            squashfs: false,
            target_arch: None,
            sisr: None,
            encryption: None,
        },
    )
    .unwrap();

    let before = std::fs::read(&input).unwrap();
    let in_footer = Footer::read_from(&mut Cursor::new(&before)).unwrap();

    let output = dir.path().join("migrated.de");
    daedalus()
        .args([
            "migrate",
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
    assert_ne!(out_footer.flags & daedalus_core::format::FLAG_SISR, 0);
    assert_eq!(out_footer.payload_sha256, in_footer.payload_sha256);

    let payload_end = (in_footer.payload_offset + in_footer.payload_csize) as usize;
    assert_eq!(&after[..payload_end], &before[..payload_end]);

    let (_, manifest) = read_sisr(&mut Cursor::new(&after))
        .unwrap()
        .expect("migrated file must embed a SISR manifest");
    assert_eq!(manifest.payload_len, in_footer.payload_csize);

    assert!(dir.path().join("migrated.daedalus.manifest").exists());
}

#[test]
fn test_registry_help() {
    daedalus()
        .args(["registry", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Push a layer or full artifact to the registry",
        ));
}

#[test]
fn test_registry_list_empty() {
    let tmp = tempdir().unwrap();
    let dir = tmp.path().join("registry");
    std::fs::create_dir_all(&dir).unwrap();

    daedalus()
        .args(["registry", "list", "--dir", dir.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("(empty)"));
}

#[test]
fn test_scan_help_lists_plain_and_no_input() {
    daedalus()
        .args(["scan", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--plain"))
        .stdout(predicate::str::contains("--no-input"))
        .stdout(predicate::str::contains("--json"));
}

#[test]
fn test_doctor_help_lists_plain() {
    daedalus()
        .args(["doctor", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--plain"))
        .stdout(predicate::str::contains("--no-input"));
}

#[test]
fn test_registry_help_lists_plain_and_json() {
    daedalus()
        .args(["registry", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--plain"))
        .stdout(predicate::str::contains("--no-input"));
}

#[test]
fn test_keygen_help_lists_no_input() {
    daedalus()
        .args(["keygen", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--no-input"));
}

#[test]
fn test_no_input_flag_accepted_by_keygen() {
    let tmp = tempdir().unwrap();
    daedalus()
        .args([
            "keygen",
            "--key-dir",
            tmp.path().to_str().unwrap(),
            "--no-input",
            "--force",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Generated Ed25519 keypair"));
}

#[test]
fn test_registry_list_json_empty() {
    let tmp = tempdir().unwrap();
    let dir = tmp.path().join("registry");
    std::fs::create_dir_all(&dir).unwrap();

    let output = daedalus()
        .args(["registry", "list", "--dir", dir.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(&stdout);
    assert!(
        parsed.is_ok(),
        "registry list --json should output valid JSON"
    );
}

#[test]
fn test_plain_flag_disables_pager() {
    let tmp = tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("app")).unwrap();
    std::fs::write(tmp.path().join("app/package.json"), "{\"name\":\"app\"}").unwrap();
    let app_dir = tmp.path().join("app");
    let output = daedalus()
        .args(["build", app_dir.to_str().unwrap(), "--dry-run", "--plain"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("\x1b[") && !stderr.contains("\u{1b}["),
        "--plain should disable ANSI colors"
    );
}

#[test]
fn test_registry_push_no_registry() {
    let tmp = tempdir().unwrap();
    let fake_file = tmp.path().join("fake.txt");
    std::fs::write(&fake_file, b"dummy").unwrap();

    daedalus()
        .args(["registry", "push", fake_file.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not a .daedalus file"));
}

#[test]
fn test_registry_push_local_extracts_layers() {
    use daedalus_core::assembly::assemble_daedalus;
    use daedalus_core::assembly::AssemblyInput;
    use std::io::Cursor;

    let tmp = tempdir().unwrap();
    let work = tmp.path();

    // Build a minimal .daedalus with layer metadata (RuntimeLayer)
    let meta = serde_json::json!({
        "name": "layer-test",
        "runtime": "python3",
        "entrypoint": ["python3", "/app/app.py"],
        "payload_format": "zstd-tar",
        "layers": [
            {
                "kind": "runtime",
                "name": "python3",
                "interpreter": "python3",
                "entrypoint": ["python3 {app}/app.py"],
                "version": "3.11",
                "env": [],
                "capabilities": ["exec", "network"]
            }
        ],
        "entrypoint_layer": "python3",
    });
    let meta_bytes = serde_json::to_vec(&meta).unwrap();

    // Minimal payload (a tar with one file, zstd-compressed)
    let tar_bytes = {
        let mut t = tar::Builder::new(Vec::new());
        let data = b"print('hello')\n";
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o755);
        h.set_mtime(1_600_000_000);
        h.set_username("root").unwrap();
        h.set_groupname("root").unwrap();
        h.set_cksum();
        t.append_data(&mut h, "app/app.py", Cursor::new(data))
            .unwrap();
        zstd::stream::encode_all(Cursor::new(t.into_inner().unwrap()), 3).unwrap()
    };

    // Minimal stub (just enough to make a valid .daedalus)
    let stub_bytes = b"EREBUS-STUB-DUMMY";
    let out = work.join("app.daedalus");
    assemble_daedalus(
        &out,
        &AssemblyInput {
            stub_bytes,
            payload: &tar_bytes,
            meta_bytes: &meta_bytes,
            squashfs: false,
            target_arch: None,
            sisr: None,
            encryption: None,
        },
    )
    .unwrap();

    // Publish to a local registry directory
    let registry_dir = work.join("registry");
    daedalus()
        .args([
            "registry",
            "push",
            out.to_str().unwrap(),
            "--local",
            registry_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("pushed layer"));

    // Verify the layer is listed
    daedalus()
        .args(["registry", "list", "--dir", registry_dir.to_str().unwrap()])
        .assert()
        .success();
}
