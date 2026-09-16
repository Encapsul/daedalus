//! Shared helpers for daedalus-cli integration tests.

#[cfg(unix)]
use std::path::PathBuf;

/// Locate a runnable stub, skipping when none can be produced so CI without a
/// full Rust/musl toolchain still passes.
///
/// Returns a PRIVATE copy of the stub binary in a per-call temp dir. Multiple
/// test threads spawn `cargo build -p daedalus-stub` concurrently; pointing
/// `DAEDALUS_STUB_PATH` at the shared `target/debug/daedalus-stub` races with
/// those builds replacing the binary mid-run. A private copy makes each test
/// immune to that.
#[cfg(unix)]
pub fn locate_stub() -> Option<PathBuf> {
    let src = locate_stub_src()?;
    let dir = tempfile::tempdir().ok()?.keep();
    let dst = dir.join("daedalus-stub");
    std::fs::copy(&src, &dst).ok()?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755)).ok()?;
    Some(dst)
}

#[cfg(unix)]
fn locate_stub_src() -> Option<PathBuf> {
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
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("manifest dir has parent")
                .join("target")
        });
    let stub = target.join("debug/daedalus-stub");
    stub.is_file().then_some(stub)
}
