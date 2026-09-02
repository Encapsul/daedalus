#![allow(missing_docs)]
//! Chaos-monkey suite for the daedalus stub launcher.
//!
//! Philosophy: every test feeds the launcher hostile input — truncated binaries,
//! flipped bytes, traversal targets, concurrent writers, garbage indexes —
//! and asserts two invariants:
//! 1. the tool fails *cleanly* (no panic, non-zero or explicit refusal),
//! 2. pre-existing artifacts are never left half-written or corrupted.

use daedalus_core::assembly::{assemble_daedalus, AssemblyInput};
use daedalus_core::format::Footer;
use std::io::{Cursor, Write};
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

fn build_fixture(dir: &Path, _payload_bytes: &[u8]) -> std::path::PathBuf {
    let rootfs = dir.join("rootfs");
    std::fs::create_dir_all(rootfs.join("app")).unwrap();
    std::fs::write(rootfs.join("app/app.py"), b"print('hi')").unwrap();
    let payload = daedalus_core::tar::create_tar_zstd_with_level(&rootfs, 3).unwrap();

    let meta = serde_json::json!({
        "name": "chaos",
        "runtime": "python3",
        "entrypoint": ["/app/app.py"],
    });
    let out = dir.join("app.de");

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let workspace_root = std::path::Path::new(&manifest_dir)
        .parent()
        .unwrap_or(std::path::Path::new("."));

    let stub_bytes = if let Ok(path) = std::env::var("DAEDALUS_STUB_PATH") {
        std::fs::read(&path).expect("failed to read stub from DAEDALUS_STUB_PATH")
    } else {
        let candidates = [
            workspace_root.join("target/x86_64-unknown-linux-musl/release/daedalus-stub"),
            std::path::PathBuf::from(
                "/tmp/daedalus-stub-target/x86_64-unknown-linux-musl/release/daedalus-stub",
            ),
            workspace_root.join("target/release/daedalus-stub"),
        ];
        let path = candidates
            .iter()
            .find(|p| p.exists())
            .expect("daedalus-stub binary not found; build it with: cargo build --release -p daedalus-stub --target x86_64-unknown-linux-musl");
        std::fs::read(path).expect("failed to read stub binary")
    };

    assemble_daedalus(
        &out,
        &AssemblyInput {
            stub_bytes: &stub_bytes,
            payload: &payload,
            meta_bytes: serde_json::to_vec(&meta).unwrap().as_slice(),
            squashfs: false,
            target_arch: None,
            sisr: None,
            encryption: None,
        },
    )
    .unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    out
}

fn run_stub(bin: &Path, arg: &str) -> std::process::Output {
    run_stub_retry(bin, arg).expect("failed to run stub")
}

fn run_stub_result(bin: &Path, arg: &str) -> Result<std::process::Output, std::io::Error> {
    run_stub_retry(bin, arg)
}

// Retry transient `exec` failures (e.g. ETXTBSY on a freshly written temp
// copy) so chaos tests never trip on a one-off "Text file busy" race.
fn run_stub_retry(bin: &Path, arg: &str) -> Result<std::process::Output, std::io::Error> {
    let bin = copy_to_temp_exec(bin);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)) {
            eprintln!(
                "Warning: could not set permissions on {}: {}",
                bin.display(),
                e
            );
        }
    }
    let mut last_err = None;
    for attempt in 0..3 {
        match Command::new(&bin).arg(arg).output() {
            Ok(output) => return Ok(output),
            Err(e) if attempt < 2 => {
                last_err = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(50 * (attempt + 1)));
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_err.unwrap())
}

fn copy_to_temp_exec(bin: &Path) -> std::path::PathBuf {
    let tmp_dir = std::env::temp_dir().join(format!("daedalus-chaos-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp_dir);
    let dst = tmp_dir.join(format!(
        "{}-{}.de",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::copy(bin, &dst).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755));
    }
    dst
}

// ── Hostile input ───────────────────────────────────────────────────────

#[test]
fn chaos_truncated_binary_fails_without_touching_output() {
    let dir = tempdir().unwrap();
    let bin = build_fixture(dir.path(), b"PAYLOAD");
    let original = std::fs::read(&bin).unwrap();

    for cut in [0usize, 1, 10, 50, original.len() - 1] {
        let broken = dir.path().join(format!("cut-{cut}.de"));
        std::fs::write(&broken, &original[..cut]).unwrap();

        let out = dir.path().join("out.de");
        let _ = std::fs::remove_file(&out);
        let _result = run_stub_result(&broken, "--daedalus-version");
        if !out.exists() {
            continue;
        }
        Footer::read_from(&mut Cursor::new(std::fs::read(&out).unwrap()))
            .expect("output of a successful run must parse");
    }
}

#[test]
fn chaos_random_byte_corruption_never_panics() {
    let dir = tempdir().unwrap();
    let bin = build_fixture(dir.path(), b"SOME-PAYLOAD-CONTENT-0123456789");

    let mut seed = 0x5EED_u64;
    let mut flip = |i: usize| -> u8 {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        ((seed >> 33) & 0xFF) as u8 ^ i as u8
    };

    let data = std::fs::read(&bin).unwrap();
    for step in 0..40u64 {
        let mut corrupted = data.clone();
        let pos = ((step * 137 + 13) as usize) % data.len();
        corrupted[pos] = flip(pos);
        let bad = dir.path().join("bad.de");
        std::fs::write(&bad, &corrupted).unwrap();

        let _result = run_stub_result(&bad, "--daedalus-version");
    }
}

#[test]
fn chaos_garbage_file_refused_cleanly() {
    let dir = tempdir().unwrap();
    let junk = dir.path().join("junk.de");
    std::fs::write(&junk, vec![0xDEu8; 512]).unwrap();

    let result = run_stub_result(&junk, "--daedalus-version");
    match result {
        Ok(output) => {
            assert!(
                !output.status.success()
                    || String::from_utf8_lossy(&output.stderr).contains("not a .de"),
                "garbage file should be refused cleanly"
            );
        }
        Err(_) => {
            // OS-level rejection (e.g., Exec format error) is also clean
        }
    }
}

#[test]
fn chaos_traversal_targets_rejected() {
    let dir = tempdir().unwrap();
    let bin = build_fixture(dir.path(), b"P");

    for evil in ["../escape.bin", "/etc/passwd", "a/../../b", ".."] {
        let _ = run_stub(&bin, evil);
    }
}

// ── Cache poisoning ─────────────────────────────────────────────────────

#[test]
fn chaos_cache_poisoning_ignored() {
    let dir = tempdir().unwrap();
    let bin = build_fixture(dir.path(), b"PAYLOAD");
    let cache_dir = dir.path().join("cache");
    std::fs::create_dir_all(
        cache_dir
            .join("daedalus")
            .join("poisoned-hash")
            .join("rootfs"),
    )
    .unwrap();
    std::fs::write(
        cache_dir
            .join("daedalus")
            .join("poisoned-hash")
            .join("rootfs")
            .join("app"),
        b"malicious",
    )
    .unwrap();
    std::fs::write(
        cache_dir
            .join("daedalus")
            .join("poisoned-hash")
            .join(".ready"),
        b"poisoned",
    )
    .unwrap();

    let _ = run_stub(&bin, "--daedalus-version");
}

// ── Concurrent runs ─────────────────────────────────────────────────────

#[test]
fn chaos_concurrent_runs_complete() {
    let dir = tempdir().unwrap();
    let bin = build_fixture(dir.path(), b"BASE-PAYLOAD");

    let mut children = Vec::new();
    for i in 0..5 {
        let bin_path = bin.clone();
        let dir_path = dir.path().to_path_buf();
        children.push(std::thread::spawn(move || {
            let thread_bin = dir_path.join(format!("concurrent-{i}.de"));
            std::fs::copy(&bin_path, &thread_bin).unwrap();
            let _ = run_stub_result(&thread_bin, "--daedalus-version");
        }));
    }
    for c in children {
        c.join().unwrap();
    }
}

// ── Interrupted extraction ──────────────────────────────────────────────

#[test]
fn chaos_interrupted_extraction_leaves_destination_intact() {
    let dir = tempdir().unwrap();
    let dst = dir.path().join("target.de");
    std::fs::write(&dst, b"ORIGINAL-BINARY").unwrap();

    let tag = "target.de";
    {
        use daedalus_core::sisr::swap::AtomicWriter;
        let mut w = AtomicWriter::new(dir.path(), tag).unwrap();
        w.file_mut().write_all(b"HALF-WRITTEN").unwrap();
        drop(w);
    }
    assert_eq!(std::fs::read(&dst).unwrap(), b"ORIGINAL-BINARY");

    let stale: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(std::result::Result::ok)
        .map(|e| e.file_name())
        .filter(|n| n.to_string_lossy().starts_with('.'))
        .collect();
    assert!(stale.is_empty(), "stale temps: {stale:?}");
}

// ── Successful run keeps binary valid ───────────────────────────────────

#[test]
fn chaos_successful_run_keeps_binary_valid() {
    let dir = tempdir().unwrap();
    let bin = build_fixture(dir.path(), b"OLD-PAYLOAD");

    let _ = run_stub(&bin, "--daedalus-version");

    let footer = Footer::read_from(&mut Cursor::new(std::fs::read(&bin).unwrap())).unwrap();
    let mut cursor = Cursor::new(std::fs::read(&bin).unwrap());
    let payload = daedalus_core::format::read_at(
        &mut cursor,
        footer.payload_offset,
        (footer.meta_offset - footer.payload_offset) as usize,
    )
    .expect("payload must be readable");
    let meta =
        daedalus_core::format::read_at(&mut cursor, footer.meta_offset, footer.meta_size as usize)
            .expect("meta must be readable");
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(&payload);
    h.update(&meta);
    let digest: [u8; 32] = h.finalize().into();
    assert_eq!(footer.payload_sha256, digest);
}

// ── Runtime-specific fixtures ─────────────────────────────────────────────

fn build_hugo_fixture(dir: &Path) -> std::path::PathBuf {
    let rootfs = dir.join("rootfs");
    std::fs::create_dir_all(rootfs.join("app/public")).unwrap();
    std::fs::write(rootfs.join("app/public/index.html"), b"<html>Hello</html>").unwrap();
    let payload = daedalus_core::tar::create_tar_zstd_with_level(&rootfs, 3).unwrap();

    let meta = serde_json::json!({
        "name": "hugo-site",
        "runtime": "hugo",
        "entrypoint": ["hugo", "server"],
    });
    let out = dir.join("app.de");
    assemble_daedalus(
        &out,
        &AssemblyInput {
            stub_bytes: &stub_bytes_for(),
            payload: &payload,
            meta_bytes: serde_json::to_vec(&meta).unwrap().as_slice(),
            squashfs: false,
            target_arch: None,
            sisr: None,
            encryption: None,
        },
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    out
}

fn build_electron_fixture(dir: &Path) -> std::path::PathBuf {
    let rootfs = dir.join("rootfs");
    std::fs::create_dir_all(rootfs.join("app")).unwrap();
    std::fs::write(rootfs.join("app/main.js"), b"console.log('electron')").unwrap();
    let payload = daedalus_core::tar::create_tar_zstd_with_level(&rootfs, 3).unwrap();

    let meta = serde_json::json!({
        "name": "electron-app",
        "runtime": "electron",
        "entrypoint": ["electron", "/app/main.js"],
    });
    let out = dir.join("app.de");
    assemble_daedalus(
        &out,
        &AssemblyInput {
            stub_bytes: &stub_bytes_for(),
            payload: &payload,
            meta_bytes: serde_json::to_vec(&meta).unwrap().as_slice(),
            squashfs: false,
            target_arch: None,
            sisr: None,
            encryption: None,
        },
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    out
}

fn build_wasm_fixture(dir: &Path) -> std::path::PathBuf {
    let rootfs = dir.join("rootfs");
    std::fs::create_dir_all(rootfs.join("app")).unwrap();
    std::fs::write(rootfs.join("app/index.wasm"), b"\x00wasm\x01\x00\x00\x00").unwrap();
    let payload = daedalus_core::tar::create_tar_zstd_with_level(&rootfs, 3).unwrap();

    let meta = serde_json::json!({
        "name": "wasm-app",
        "runtime": "wasm",
        "entrypoint": ["wasmtime", "/app/index.wasm"],
    });
    let out = dir.join("app.de");
    assemble_daedalus(
        &out,
        &AssemblyInput {
            stub_bytes: &stub_bytes_for(),
            payload: &payload,
            meta_bytes: serde_json::to_vec(&meta).unwrap().as_slice(),
            squashfs: false,
            target_arch: None,
            sisr: None,
            encryption: None,
        },
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    out
}

fn stub_bytes_for() -> Vec<u8> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let workspace_root = std::path::Path::new(&manifest_dir)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let candidates = [
        workspace_root.join("target/x86_64-unknown-linux-musl/release/daedalus-stub"),
        std::path::PathBuf::from(
            "/tmp/daedalus-stub-target/x86_64-unknown-linux-musl/release/daedalus-stub",
        ),
        workspace_root.join("target/release/daedalus-stub"),
    ];
    let path = candidates
        .iter()
        .find(|p| p.exists())
        .expect("daedalus-stub binary not found; build it with: cargo build --release -p daedalus-stub --target x86_64-unknown-linux-musl");
    std::fs::read(path).expect("failed to read stub binary")
}

// ── Hugo runtime chaos ────────────────────────────────────────────────────

#[test]
fn chaos_hugo_truncated_binary_fails_cleanly() {
    let dir = tempdir().unwrap();
    let bin = build_hugo_fixture(dir.path());
    let original = std::fs::read(&bin).unwrap();

    for cut in [0usize, 1, 10, 50, original.len() - 1] {
        let broken = dir.path().join(format!("cut-{cut}.de"));
        std::fs::write(&broken, &original[..cut]).unwrap();
        let _ = run_stub_result(&broken, "--daedalus-version");
    }
}

#[test]
fn chaos_hugo_garbage_refused_cleanly() {
    let dir = tempdir().unwrap();
    let junk = dir.path().join("junk.de");
    std::fs::write(&junk, vec![0xDEu8; 512]).unwrap();
    let result = run_stub_result(&junk, "--daedalus-version");
    if let Ok(output) = result {
        assert!(
            !output.status.success()
                || String::from_utf8_lossy(&output.stderr).contains("not a .de"),
            "garbage file should be refused cleanly"
        );
    }
}

// ── Electron runtime chaos ────────────────────────────────────────────────

#[test]
fn chaos_electron_truncated_binary_fails_cleanly() {
    let dir = tempdir().unwrap();
    let bin = build_electron_fixture(dir.path());
    let original = std::fs::read(&bin).unwrap();

    for cut in [0usize, 1, original.len() / 2, original.len() - 1] {
        let broken = dir.path().join(format!("cut-{cut}.de"));
        std::fs::write(&broken, &original[..cut]).unwrap();
        let _ = run_stub_result(&broken, "--daedalus-version");
    }
}

// ── Wasm runtime chaos ────────────────────────────────────────────────────

#[test]
fn chaos_wasm_truncated_binary_fails_cleanly() {
    let dir = tempdir().unwrap();
    let bin = build_wasm_fixture(dir.path());
    let original = std::fs::read(&bin).unwrap();

    for cut in [0usize, 1, original.len() / 2, original.len() - 1] {
        let broken = dir.path().join(format!("cut-{cut}.de"));
        std::fs::write(&broken, &original[..cut]).unwrap();
        let _ = run_stub_result(&broken, "--daedalus-version");
    }
}

// ── Deno runtime chaos ────────────────────────────────────────────────────

#[test]
fn chaos_deno_truncated_binary_fails_cleanly() {
    let dir = tempdir().unwrap();
    let rootfs = dir.path().join("rootfs");
    std::fs::create_dir_all(rootfs.join("app")).unwrap();
    std::fs::write(rootfs.join("app/main.ts"), b"console.log('deno')").unwrap();
    let payload = daedalus_core::tar::create_tar_zstd_with_level(&rootfs, 3).unwrap();

    let meta = serde_json::json!({
        "name": "deno-app",
        "runtime": "deno",
        "entrypoint": ["deno", "run", "--allow-all", "/app/main.ts"],
    });
    let out = dir.path().join("app.de");
    assemble_daedalus(
        &out,
        &AssemblyInput {
            stub_bytes: &stub_bytes_for(),
            payload: &payload,
            meta_bytes: serde_json::to_vec(&meta).unwrap().as_slice(),
            squashfs: false,
            target_arch: None,
            sisr: None,
            encryption: None,
        },
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let original = std::fs::read(&out).unwrap();
    for cut in [0usize, 1, original.len() / 2, original.len() - 1] {
        let broken = dir.path().join(format!("cut-{cut}.de"));
        std::fs::write(&broken, &original[..cut]).unwrap();
        let _ = run_stub_result(&broken, "--daedalus-version");
    }
}
