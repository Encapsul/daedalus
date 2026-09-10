//! End-to-end tests for default signing: `daedalus build` signs every artifact
//! with a self-generated, self-trusted dev key, so `daedalus verify` authenticates
//! the result out of the box — unless `--skip-sign` is given.
//!
//! All tests isolate their key/trust state via `XDG_DATA_HOME` (keys land in
//! `$XDG_DATA_HOME/daedalus/keys`) and `DAEDALUS_TRUSTED_DIR` (trust anchor) so
//! nothing leaks into a real user's `~/.local/share/daedalus` or
//! `~/.daedalus/trusted-keys`.

use assert_cmd::Command;
use daedalus_core::assembly::{assemble_daedalus, AssemblyInput};
use predicates::prelude::*;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn daedalus() -> Command {
    let mut cmd = Command::cargo_bin("daedalus").unwrap();
    cmd.env("NO_COLOR", "1");
    cmd
}

/// Isolate the dev-key location and trust anchor for a new command.
/// `default_key_dir()` = `$XDG_DATA_HOME/daedalus/keys`; `trusted_keys_dir()`
/// honors `$DAEDALUS_TRUSTED_DIR` directly.
fn isolated(home: &Path, trust_dir: &Path) -> Command {
    let mut cmd = daedalus();
    cmd.env("XDG_DATA_HOME", home)
        .env("DAEDALUS_TRUSTED_DIR", trust_dir);
    cmd
}

/// The dev-key directory a `home` XDG base implies.
fn dev_key_dir(home: &Path) -> PathBuf {
    home.join("daedalus").join("keys")
}

/// A minimal binary-runtime app dir with an executable `app/app` that prints a
/// marker on stdout (so at-rest tests can tell a successful exec from a
/// refusal). Runtime detection for Binary requires actual ELF bytes, so
/// compile with `cc`. Returns `None` when no C compiler is available (test
/// skips).
fn app_dir(root: &Path) -> Option<PathBuf> {
    std::fs::create_dir_all(root).unwrap();
    let src = root.join("h.c");
    std::fs::write(
        &src,
        "#include <stdio.h>\nint main(void){printf(\"hello-from-at-rest-binary\\n\");return 0;}\n",
    )
    .ok()?;
    let bin = root.join("app");
    let ok = std::process::Command::new("cc")
        .arg(&src)
        .arg("-o")
        .arg(&bin)
        .status()
        .ok()?
        .success();
    if !ok {
        return None;
    }
    let app = root.join("appdir");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::copy(&bin, app.join("app")).unwrap();
    std::fs::set_permissions(app.join("app"), PermissionsExt::from_mode(0o755)).unwrap();
    Some(app)
}

/// Run an assembled artifact like a user would, with fully isolated runtime
/// state (cache, data, HOME) and the given trust anchor.
fn run_artifact(bin: &Path, home: &Path, trust_dir: &Path) -> std::process::Output {
    std::fs::set_permissions(bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::create_dir_all(home.join("run-cache")).unwrap();
    std::fs::create_dir_all(home.join("run-data")).unwrap();
    std::fs::create_dir_all(home.join("run-home")).unwrap();
    std::process::Command::new(bin)
        .env("XDG_CACHE_HOME", home.join("run-cache"))
        .env("XDG_DATA_HOME", home.join("run-data"))
        .env("HOME", home.join("run-home"))
        .env("DAEDALUS_TRUSTED_DIR", trust_dir)
        .output()
        .expect("failed to run the assembled artifact")
}

/// A valid unsigned `.de` with a fake stub (never executed — verify only reads
/// the footer), for signing without a full build.
fn fake_de(dir: &Path, name: &str) -> PathBuf {
    let payload = daedalus_core::compress::compress(b"not-a-real-layer").unwrap();
    let meta = serde_json::json!({
        "name": "fake",
        "runtime": "binary",
        "entrypoint": ["/app/app"],
        "isolation": 0,
        "layers": [],
    });
    let out = dir.join(name);
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

#[test]
fn build_is_signed_by_default_and_verifies_with_auto_trusted_key() {
    let Some(stub) = locate_stub() else {
        eprintln!("skipping: no daedalus-stub available");
        return;
    };

    let home = tempfile::tempdir().unwrap();
    let trust_dir = home.path().join("trusted");
    std::fs::create_dir_all(&trust_dir).unwrap();
    let Some(app) = app_dir(&home.path().join("src")) else {
        eprintln!("skipping: no C compiler available");
        return;
    };

    // Build WITHOUT --key and WITHOUT --skip-sign: default signing must kick in.
    let out = home.path().join("app.de");
    isolated(home.path(), &trust_dir)
        .args([
            "build",
            app.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--isolation",
            "0",
        ])
        .env("DAEDALUS_STUB_PATH", &stub)
        .assert()
        .success();

    // A dev key (and matching pub) must have been generated in the XDG key dir.
    let key_dir = dev_key_dir(home.path());
    let keys = std::fs::read_dir(&key_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect::<Vec<_>>();
    assert!(
        keys.iter()
            .any(|p| p.extension().is_some_and(|e| e == "key")),
        "default build must generate a dev key in {}",
        key_dir.display()
    );

    // And `verify` must authenticate this build out of the box.
    isolated(home.path(), &trust_dir)
        .args(["verify", out.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn skip_sign_produces_unsigned_binary() {
    let Some(stub) = locate_stub() else {
        eprintln!("skipping: no daedalus-stub available");
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let trust_dir = home.path().join("trusted");
    std::fs::create_dir_all(&trust_dir).unwrap();
    let Some(app) = app_dir(&home.path().join("src")) else {
        eprintln!("skipping: no C compiler available");
        return;
    };

    let out = home.path().join("app.de");
    isolated(home.path(), &trust_dir)
        .args([
            "build",
            app.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--isolation",
            "0",
            "--skip-sign",
        ])
        .env("DAEDALUS_STUB_PATH", &stub)
        .assert()
        .success();

    // `verify` must report the artifact is not signed (informational, exit 0).
    isolated(home.path(), &trust_dir)
        .args(["verify", out.to_str().unwrap(), "--no-input"])
        .assert()
        .success()
        .stderr(predicates::str::contains("not signed"));
}

#[test]
fn verify_rejects_signed_binary_without_trusted_key() {
    let home = tempfile::tempdir().unwrap();
    let trust_dir = home.path().join("trusted");
    std::fs::create_dir_all(&trust_dir).unwrap();

    // Sign with `daedalus sign` using a generated key whose pubkey is NOT used.
    let de = fake_de(home.path(), "in.de");
    let key_dir = home.path().join("manual-keys");
    isolated(home.path(), &trust_dir)
        .args(["keygen", "--key-dir", key_dir.to_str().unwrap()])
        .assert()
        .success();
    let key_file = std::fs::read_dir(&key_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "key"))
        .unwrap();
    let signed = home.path().join("signed.de");
    isolated(home.path(), &trust_dir)
        .args([
            "sign",
            de.to_str().unwrap(),
            "--key",
            key_file.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .success();

    // verify with an empty trust anchor must reject (no auto-trust happened).
    isolated(home.path(), &trust_dir)
        .args(["verify", de.to_str().unwrap(), "--no-input"])
        .assert()
        .failure();
}

#[test]
fn signed_binary_refuses_to_run_without_trusted_key_at_rest() {
    let Some(stub) = locate_stub() else {
        eprintln!("skipping: no daedalus-stub available");
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let build_trust = home.path().join("trusted");
    std::fs::create_dir_all(&build_trust).unwrap();
    let Some(app) = app_dir(&home.path().join("src")) else {
        eprintln!("skipping: no C compiler available");
        return;
    };

    let out = home.path().join("app.de");
    isolated(home.path(), &build_trust)
        .args([
            "build",
            app.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--isolation",
            "0",
        ])
        .env("DAEDALUS_STUB_PATH", &stub)
        .assert()
        .success();

    // Run with a DIFFERENT, empty trust anchor: the stub must refuse at cold
    // start (at-rest authenticity), never exec the app.
    let empty_trust = home.path().join("empty-trusted");
    std::fs::create_dir_all(&empty_trust).unwrap();
    let run = run_artifact(&out, home.path(), &empty_trust);
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    assert!(
        !run.status.success(),
        "untrusted at-rest run must be refused, stderr: {stderr}"
    );
    assert!(
        !String::from_utf8_lossy(&run.stdout).contains("hello-from-at-rest-binary"),
        "refused run must not exec the app, stderr: {stderr}"
    );
}

#[test]
fn signed_binary_runs_when_key_is_shared_trust_at_rest() {
    let Some(stub) = locate_stub() else {
        eprintln!("skipping: no daedalus-stub available");
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let trust_dir = home.path().join("trusted");
    std::fs::create_dir_all(&trust_dir).unwrap();
    let Some(app) = app_dir(&home.path().join("src")) else {
        eprintln!("skipping: no C compiler available");
        return;
    };

    let out = home.path().join("app.de");
    isolated(home.path(), &trust_dir)
        .args([
            "build",
            app.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--isolation",
            "0",
        ])
        .env("DAEDALUS_STUB_PATH", &stub)
        .assert()
        .success();

    // Same trust anchor the build self-trusted its dev key into: at-rest
    // verification passes and the app runs.
    let run = run_artifact(&out, home.path(), &trust_dir);
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    assert!(
        run.status.success(),
        "trusted at-rest run must succeed, stderr: {stderr}"
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("hello-from-at-rest-binary"),
        "trusted at-rest run must exec the app, stderr: {stderr}"
    );
}

#[test]
fn skip_sign_binary_still_runs_legacy_without_trust() {
    let Some(stub) = locate_stub() else {
        eprintln!("skipping: no daedalus-stub available");
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let trust_dir = home.path().join("trusted");
    std::fs::create_dir_all(&trust_dir).unwrap();
    let Some(app) = app_dir(&home.path().join("src")) else {
        eprintln!("skipping: no C compiler available");
        return;
    };

    let out = home.path().join("app.de");
    isolated(home.path(), &trust_dir)
        .args([
            "build",
            app.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--isolation",
            "0",
            "--skip-sign",
        ])
        .env("DAEDALUS_STUB_PATH", &stub)
        .assert()
        .success();

    // Legacy behavior: unsigned binaries skip at-rest verification entirely and
    // run even against an empty trust anchor.
    let empty_trust = home.path().join("empty-trusted");
    std::fs::create_dir_all(&empty_trust).unwrap();
    let run = run_artifact(&out, home.path(), &empty_trust);
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    assert!(
        run.status.success(),
        "unsigned legacy run must succeed, stderr: {stderr}"
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("hello-from-at-rest-binary"),
        "unsigned legacy run must exec the app, stderr: {stderr}"
    );
}

/// Uses the same stub-location strategy as `stdio_flow.rs`.
fn locate_stub() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("DAEDALUS_STUB_PATH") {
        let p = PathBuf::from(path);
        if p.is_file() {
            return Some(p);
        }
    }
    let ok = std::process::Command::new("cargo")
        .args(["build", "-q", "-p", "daedalus-stub"])
        .status()
        .ok()?
        .success();
    if !ok {
        return None;
    }
    let target = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("manifest dir has parent")
                .join("target")
        });
    let stub = target.join("debug/daedalus-stub");
    stub.is_file().then_some(stub)
}
