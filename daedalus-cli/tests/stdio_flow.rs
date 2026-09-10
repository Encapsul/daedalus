//! End-to-end tests for the `-` stdin/stdout convention across commands.
//!
//! `-` lets a `.de` binary be piped instead of named on disk:
//!   build -o -      emits the artifact on stdout
//!   run -           executes a binary streamed in on stdin
//!   inspect -       reads stdin
//!   sign -          reads stdin, writes signed bytes to stdout
//!   verify -        reads stdin
//!   swap - - -o -   reads binary+replacement from args, binary from stdin, output to stdout
//!
//! Read-command tests synthesize a valid `.de` with `assemble_daedalus` so they
//! do not require a self-extracting stub; the `build -o -` test needs a real
//! stub and skips gracefully when none is available.

use assert_cmd::Command;
use daedalus_core::assembly::{assemble_daedalus, AssemblyInput};
use predicates::prelude::*;
use std::path::{Path, PathBuf};

fn daedalus() -> Command {
    let mut cmd = Command::cargo_bin("daedalus").unwrap();
    cmd.env("NO_COLOR", "1");
    cmd
}

/// Build a zstd-tar payload containing `app/app` (so swap can find its layer)
/// and a `.daedalus` footer over it. The stub bytes are arbitrary — only the
/// footer + hash matter for inspect/sign/verify; swap reassembles from the
/// embedded stub bytes without executing them.
fn make_de(dir: &Path, name: &str) -> PathBuf {
    let mut tar_buf = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_buf);
        let mut header = tar::Header::new_gnu();
        header.set_size(4);
        header.set_mode(0o755);
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        header.set_entry_type(tar::EntryType::Regular);
        builder
            .append_data(&mut header, "app/app", "orig".as_bytes())
            .unwrap();
        builder.finish().unwrap();
    }
    let payload = daedalus_core::compress::compress(&tar_buf).unwrap();

    let meta = serde_json::json!({
        "name": "test-app",
        "runtime": "binary",
        "version": "1.0.0",
        "entrypoint": ["/app/app"],
        "isolation": 0,
        "layers": [
            {"name": "app", "usize": 4},
        ],
    });
    let stub = b"FAKE-STUB-NOT-EXECUTABLE".to_vec();
    let out = dir.join(name);
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

/// Generate a signing keypair into `dir`, returns the private key path.
fn keygen(dir: &Path) -> PathBuf {
    daedalus()
        .args([
            "keygen",
            "--key-dir",
            dir.to_str().unwrap(),
            "--no-input",
            "--force",
        ])
        .assert()
        .success();
    // keygen writes <name>.key (private) and <name>.pub (public) into dir.
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "key"))
        .expect("keygen produced a .key")
}

#[test]
fn inspect_reads_de_from_stdin() {
    let tmp = tempfile::tempdir().unwrap();
    let de = make_de(tmp.path(), "app.de");
    let bytes = std::fs::read(&de).unwrap();

    daedalus()
        .arg("inspect")
        .arg("-")
        .write_stdin(bytes)
        .assert()
        .success()
        .stdout(predicate::str::contains("test-app"));
}

#[test]
fn inspect_from_stdin_json_reports_stdin() {
    let tmp = tempfile::tempdir().unwrap();
    let de = make_de(tmp.path(), "app.de");
    let bytes = std::fs::read(&de).unwrap();
    let out = daedalus()
        .args(["inspect", "-", "--json"])
        .write_stdin(bytes)
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["file"], "stdin");
    assert_eq!(v["meta"]["name"], "test-app");
}

#[test]
fn sign_stdin_to_stdout_produces_valid_signed_binary() {
    let tmp = tempfile::tempdir().unwrap();
    let de = make_de(tmp.path(), "app.de");
    let bytes = std::fs::read(&de).unwrap();
    let key = keygen(tmp.path());

    let out = daedalus()
        .args([
            "sign",
            "-",
            "--key",
            key.to_str().unwrap(),
            "--force",
            "--quiet",
        ])
        .write_stdin(bytes)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "sign - should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // The stdin→stdout result must itself be a valid, signed .de that a
    // follow-up `verify -` accepts.
    let signed = out.stdout;
    let verify = daedalus()
        .args([
            "verify",
            "-",
            "--trusted-dir",
            tmp.path().to_str().unwrap(),
            "--no-input",
        ])
        .write_stdin(signed)
        .output()
        .unwrap();
    assert!(
        verify.status.success(),
        "verify of piped signed binary should succeed: {}",
        String::from_utf8_lossy(&verify.stderr)
    );
}

#[test]
fn verify_unsigned_stdin_reports_not_signed() {
    let tmp = tempfile::tempdir().unwrap();
    let de = make_de(tmp.path(), "app.de");
    let bytes = std::fs::read(&de).unwrap();
    let out = daedalus()
        .args(["verify", "-", "--no-input"])
        .write_stdin(bytes)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "verify of an unsigned stdin reader should exit cleanly"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("not signed"));
}

/// Locate a runnable stub, skipping when none can be produced so CI without a
/// full Rust/musl toolchain still passes.
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

#[test]
fn build_streams_artifact_to_stdout() {
    let Some(stub) = locate_stub() else {
        eprintln!("skipping: no daedalus-stub available");
        return;
    };
    let Some(native) = compile_native_binary(tmpdir_owned()) else {
        eprintln!("skipping: no C compiler available");
        return;
    };

    // A binary-runtime "app": a compiled native executable in the app root.
    let app = native.parent().unwrap().join("app");
    std::fs::create_dir_all(&app).unwrap();
    let mut orig = std::fs::read(&native).unwrap();
    // Make sure a copy owns the bytes we write (avoid collisions in tmp).
    std::fs::write(app.join("app"), &orig).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(app.join("app"), std::fs::Permissions::from_mode(0o755)).unwrap();

    // Stream `build -o -` output to a file and confirm it is a valid .de.
    // `build` writes status messages to stderr, so stdout carries only the
    // artifact — safe to pipe with std::process::Command.
    let bin = assert_cmd::cargo::cargo_bin("daedalus");
    let iso = tmpdir_owned();
    let mut child = std::process::Command::new(bin)
        .args([
            "build",
            app.to_str().unwrap(),
            "-o",
            "-",
            "--isolation",
            "0",
        ])
        .env("DAEDALUS_STUB_PATH", &stub)
        // Isolate the default dev-key + trust state (signing is on by default).
        .env("XDG_DATA_HOME", iso.join("data"))
        .env("DAEDALUS_TRUSTED_DIR", iso.join("trusted"))
        .env("NO_COLOR", "1")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut out_bytes = Vec::new();
    std::io::Read::read_to_end(&mut child.stdout.as_mut().unwrap(), &mut out_bytes).unwrap();
    let status = child.wait().unwrap();
    assert!(status.success(), "build -o - should succeed");

    let captured = tmpdir_owned().join("captured.de");
    std::fs::write(&captured, &out_bytes).unwrap();
    // The streamed bytes must parse as a valid .de footer.
    let inspect = daedalus()
        .args(["inspect", captured.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(
        inspect.status.success(),
        "streamed build output must be a valid .de: {}",
        String::from_utf8_lossy(&inspect.stderr)
    );
}

fn tmpdir_owned() -> PathBuf {
    tempfile::tempdir().unwrap().keep()
}

/// Compile a tiny native executable (runtime-Binary detection requires an
/// ELF/PE). Returns None when no C compiler is available.
fn compile_native_binary(dir: PathBuf) -> Option<PathBuf> {
    let src = dir.join("h.c");
    std::fs::write(&src, "int main(void){return 0;}\n").ok()?;
    let out = dir.join("hello");
    let ok = std::process::Command::new("cc")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .status()
        .ok()?
        .success();
    ok.then_some(out)
}

#[test]
fn build_to_dash_respects_multi_target_rejection() {
    // Multi-target with `-o -` cannot work — two artifacts cannot share one
    // pipe. This must fail with a clear error, not write a file named `-`.
    let Some(stub) = locate_stub() else {
        eprintln!("skipping: no daedalus-stub available");
        return;
    };
    let Some(native) = compile_native_binary(tmpdir_owned()) else {
        eprintln!("skipping: no C compiler available");
        return;
    };
    let app = native.parent().unwrap().join("app");
    std::fs::create_dir_all(&app).unwrap();
    let bytes = std::fs::read(&native).unwrap();
    std::fs::write(app.join("app"), &bytes).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(app.join("app"), std::fs::Permissions::from_mode(0o755)).unwrap();

    let out = daedalus()
        .args([
            "build",
            app.to_str().unwrap(),
            "-o",
            "-",
            "--target",
            "linux-x64,linux-arm64",
            "--no-install",
            "--isolation",
            "0",
        ])
        .env("DAEDALUS_STUB_PATH", &stub)
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "multi-target -o - should be rejected"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("stdout"),
        "rejection should explain that stdout cannot hold multiple artifacts: {err}"
    );
}

#[test]
fn run_stdin_rejects_invalid_de_cleanly() {
    // `run -` with bytes that are not a valid `.de` must fail with a clear
    // error instead of attempting to exec garbage.
    let out = daedalus()
        .args(["run", "-"])
        .write_stdin(b"this is not a daedalus binary at all, far too short".to_vec())
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "run - with invalid input should fail"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("not a valid .de") || err.contains("not a valid daedalus"),
        "run should report an invalid .de: {err}"
    );
}

#[test]
fn swap_reads_stdin_and_writes_stdout() {
    let tmp = tempfile::tempdir().unwrap();
    let de = make_de(tmp.path(), "app.de");
    let bytes = std::fs::read(&de).unwrap();
    // swap replaces `app/<new-file filename>`; our payload holds `app/app`, so
    // the replacement must be named `app` for the layer lookup to match.
    let replacement = tmp.path().join("app");
    std::fs::write(&replacement, "replacement-bytes").unwrap();

    let out = daedalus()
        .args([
            "swap",
            "-",
            "app",
            replacement.to_str().unwrap(),
            "-o",
            "-",
            "--no-input",
        ])
        .write_stdin(bytes)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "swap - -> - should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let swapped = tmp.path().join("swapped.de");
    std::fs::write(&swapped, &out.stdout).unwrap();
    // The swap output must still be a valid .de.
    daedalus()
        .args(["inspect", swapped.to_str().unwrap(), "--json"])
        .assert()
        .success();
}
