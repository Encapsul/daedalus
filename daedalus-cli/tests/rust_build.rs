//! Phase 8 Step 2 end-to-end: a real Cargo project is detected, compiled by
//! `cargo build --release`, packaged, and the assembled binary must run and
//! print the app's output.
//!
//! Requires `cargo` on PATH (checked, not downloaded). The stub binary is
//! located from `DAEDALUS_STUB_PATH`, an existing build in the target dir, or a
//! one-shot `cargo build -p daedalus-stub`; the test skips with an explicit
//! message when none of these are possible.

use std::path::{Path, PathBuf};
use std::process::Command;

fn cargo_available() -> bool {
    Command::new("cargo")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The workspace target dir, resolved independently of the test's CWD.
fn workspace_target_dir() -> PathBuf {
    std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("manifest dir has a parent")
                .join("target")
        })
}

/// Locates (or builds) a runnable stub for the host. Returns `None` when no
/// stub can be produced — the caller skips instead of failing.
///
/// A plain `cargo build -p daedalus-stub` runs on every call: it is a fast
/// no-op when fresh, which GUARANTEES the embedded stub matches the current
/// sources — a stale prebuilt stub (e.g. an old musl artifact) fails with
/// confusing "unsupported runtime" errors at exec time.
fn locate_stub() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("DAEDALUS_STUB_PATH") {
        let p = PathBuf::from(path);
        if p.is_file() {
            return Some(p);
        }
    }
    let status = Command::new("cargo")
        .args(["build", "-q", "-p", "daedalus-stub"])
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    let stub = workspace_target_dir().join("debug/daedalus-stub");
    stub.is_file().then_some(stub)
}

fn write_hello_project(dir: &Path) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\n\
         name = \"hello-daedalus\"\n\
         version = \"0.1.0\"\n\
         edition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.rs"),
        "fn main() { println!(\"hello-from-daedalus-rust\"); }\n",
    )
    .unwrap();
}

#[test]
fn rust_app_is_detected_built_and_runs() {
    if !cargo_available() {
        eprintln!("skipping: cargo not on PATH");
        return;
    }
    let Some(stub) = locate_stub() else {
        eprintln!("skipping: no daedalus-stub available to assemble with");
        return;
    };
    eprintln!("using stub: {}", stub.display());

    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("hello");
    write_hello_project(&project);

    let out = tmp.path().join("hello.daedalus");
    let cli = env!("CARGO_BIN_EXE_daedalus");
    let build = Command::new(cli)
        .args([
            "build",
            project.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ])
        // NOTE: no `--no-install` — it also skips the cargo build itself
        // (same semantics as the Go path), and this test must exercise it.
        .env("DAEDALUS_STUB_PATH", &stub)
        .output()
        .expect("failed to spawn daedalus build");
    let stderr = String::from_utf8_lossy(&build.stderr).into_owned();
    assert!(
        build.status.success(),
        "daedalus build must succeed for a Rust app: {stderr}"
    );
    assert!(out.is_file(), "artifact must exist: {stderr}");

    // The assembled artifact is self-extracting: running it must extract the
    // rootfs and execvp the compiled binary.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let run = Command::new(&out)
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .env("XDG_DATA_HOME", tmp.path().join("data"))
        .env("HOME", tmp.path().join("home"))
        .output()
        .expect("failed to run the assembled Rust artifact");
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    assert!(
        run.status.success(),
        "assembled binary must exit 0: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        stdout.contains("hello-from-daedalus-rust"),
        "assembled binary must print the Rust app output, got: {stdout}"
    );
}
