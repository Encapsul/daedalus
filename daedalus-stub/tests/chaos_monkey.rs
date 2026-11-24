#![allow(missing_docs)]
#![cfg(target_os = "linux")]
//! Chaos-monkey suite for the daedalus stub launcher.
//!
//! Philosophy: every test feeds the launcher hostile input - truncated binaries,
//! flipped bytes, traversal targets, concurrent writers, garbage indexes -
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
    build_fixture_with_script(dir, b"#!/bin/sh\necho hi\n")
}

fn build_fixture_with_script(dir: &Path, script: &[u8]) -> std::path::PathBuf {
    let rootfs = dir.join("rootfs");
    std::fs::create_dir_all(rootfs.join("app")).unwrap();
    let app = rootfs.join("app/app");
    std::fs::write(&app, script).unwrap();
    set_executable(&app);

    let payload = daedalus_core::tar::create_tar_zstd_with_level(&rootfs, 3).unwrap();
    let meta = serde_json::json!({
        "name": "chaos",
        "runtime": "binary",
        "entrypoint": ["/app/app"],
    });
    let out = dir.join("app.de");
    assemble_fixture(&out, &payload, &meta);
    out
}

fn assemble_fixture(out: &Path, payload: &[u8], meta: &serde_json::Value) {
    assemble_daedalus(
        out,
        &AssemblyInput {
            stub_bytes: &stub_bytes_for(),
            payload,
            meta_bytes: serde_json::to_vec(meta).unwrap().as_slice(),
            squashfs: false,
            target_arch: None,
            sisr: None,
            encryption: None,
        },
    )
    .unwrap();
    set_executable(out);
}

fn run_stub(bin: &Path, arg: &str) -> std::process::Output {
    run_stub_result(bin, arg).expect("failed to run stub")
}

fn run_stub_result(bin: &Path, arg: &str) -> Result<std::process::Output, std::io::Error> {
    run_stub_retry(bin, Some(arg), None)
}

/// Run the packaged app itself: `--daedalus-version` exits before the
/// integrity check, so it cannot be used as a fail-closed oracle.
fn run_stub_exec(bin: &Path) -> Result<std::process::Output, std::io::Error> {
    run_stub_retry(bin, None, None)
}

fn run_stub_cached(
    bin: &Path,
    arg: Option<&str>,
    cache: &Path,
) -> Result<std::process::Output, std::io::Error> {
    run_stub_retry(bin, arg, Some(cache))
}

// Each case gets its own copy and cache root: shared /tmp paths made cache
// interference impossible to distinguish from a product bug.
fn run_stub_retry(
    bin: &Path,
    arg: Option<&str>,
    cache: Option<&Path>,
) -> Result<std::process::Output, std::io::Error> {
    let tmp = tempdir()?;
    let copy = tmp.path().join("case.de");
    std::fs::copy(bin, &copy)?;
    set_executable(&copy);
    let cache_dir = cache
        .map(Path::to_path_buf)
        .unwrap_or_else(|| tmp.path().join("cache"));
    let mut last_err = None;
    for attempt in 0..3 {
        let mut cmd = Command::new(&copy);
        if let Some(arg) = arg {
            cmd.arg(arg);
        }
        match cmd.env("XDG_CACHE_HOME", &cache_dir).output() {
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

fn set_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
    }
    #[cfg(not(unix))]
    let _ = path;
}

fn assert_no_panic(output: &std::process::Output, label: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked")
            && !stderr.contains("SIGSEGV")
            && !stderr.contains("core dumped"),
        "{label}: crash marker in stderr: {stderr}"
    );
}

fn assert_fail_closed(result: Result<std::process::Output, std::io::Error>, label: &str) {
    match result {
        // execve refusing a non-ELF file is a clean rejection.
        Err(_) => {}
        Ok(output) => {
            assert!(
                !output.status.success(),
                "{label}: corrupted bundle ran successfully"
            );
            assert_no_panic(&output, label);
        }
    }
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

        assert_fail_closed(
            run_stub_result(&broken, "--daedalus-version"),
            &format!("truncated-{cut}"),
        );
        let out = dir.path().join("out.de");
        assert!(!out.exists(), "truncated run wrote {out:?}");
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
    let footer = Footer::read_from(&mut Cursor::new(data.clone())).unwrap();
    // The integrity hash covers payload || metadata; flipping an inert stub
    // byte is legitimate and would be a false positive.
    let start = usize::try_from(footer.payload_offset).unwrap();
    let end = usize::try_from(footer.meta_offset + footer.meta_size).unwrap();
    assert!(end <= data.len() && start < end);
    for step in 0..40u64 {
        let pos = start + ((step * 137 + 13) as usize) % (end - start);
        let mut corrupted = data.clone();
        corrupted[pos] = flip(pos);
        let bad = dir.path().join("bad.de");
        std::fs::write(&bad, &corrupted).unwrap();

        assert_fail_closed(run_stub_exec(&bad), &format!("flip-{step}@{pos}"));
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
    let bin = build_fixture_with_script(dir.path(), b"#!/bin/sh\nprintf '%s\\n' \"$@\"\n");

    // The stub must forward hostile-looking args verbatim, never interpret
    // them as paths of its own.
    for evil in ["../escape.bin", "/etc/passwd", "a/../../b", ".."] {
        let output = run_stub(&bin, evil);
        assert!(output.status.success(), "app did not run for {evil:?}");
        assert_no_panic(&output, "traversal");
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), evil);
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

    let result = run_stub_cached(&bin, None, &cache_dir).expect("poisoned cache run failed");
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        result.status.success() && stdout.contains("hi") && !stdout.contains("malicious"),
        "poisoned cache was used: {stdout}"
    );
    assert_no_panic(&result, "cache-poisoning");
}

// A ready marker and a forgeable size/mtime manifest must never substitute
// for authenticating the artifact on a warm start. Tamper a payload byte,
// rewrite `.source` to match the tampered file, and require refusal at the
// warm gate rather than execution of the cached rootfs.
#[test]
fn warm_cache_hit_reverifies_source_integrity() {
    let dir = tempdir().unwrap();
    let bin = build_fixture(dir.path(), b"PAYLOAD");
    let cache_base = dir.path().join("cache");
    std::fs::create_dir_all(&cache_base).unwrap();

    let footer = {
        let mut f = std::fs::File::open(&bin).unwrap();
        Footer::read_from(&mut f).unwrap()
    };
    let hash = footer.sha256_hex();

    let live = dir.path().join("live.de");
    std::fs::copy(&bin, &live).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&live, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let first = Command::new(&live)
        .env("XDG_CACHE_HOME", &cache_base)
        .env_remove("DAEDALUS_SISR_ALLOW_UNSIGNED")
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "cold run failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let cache_root = cache_base.join("daedalus").join(&hash);
    assert!(
        cache_root.join(".ready").is_file(),
        "cache was not populated"
    );

    // Flip one payload byte; size stays identical so only the digest detects it.
    let mut bytes = std::fs::read(&live).unwrap();
    let off = usize::try_from(footer.payload_offset).unwrap() + 10;
    bytes[off] ^= 0x01;
    std::fs::write(&live, &bytes).unwrap();

    // Forge the cache hint so the warm path is actually entered.
    let md = std::fs::metadata(&live).unwrap();
    let source = serde_json::json!({
        "size": md.len(),
        "mtime_ns": md
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    });
    std::fs::write(cache_root.join(".source"), source.to_string()).unwrap();

    let second = Command::new(&live)
        .env("XDG_CACHE_HOME", &cache_base)
        .env_remove("DAEDALUS_SISR_ALLOW_UNSIGNED")
        .output()
        .unwrap();
    assert!(
        !second.status.success(),
        "tampered warm artifact was executed: {}",
        String::from_utf8_lossy(&second.stdout)
    );
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.contains("integrity") || stderr.contains("SHA-256") || stderr.contains("signature"),
        "unexpected refusal reason: {stderr}"
    );
}

#[test]
fn chaos_footer_and_metadata_corruption_rejected() {
    let dir = tempdir().unwrap();
    let bin = build_fixture(dir.path(), b"PAYLOAD");
    let original = std::fs::read(&bin).unwrap();
    let footer = Footer::read_from(&mut Cursor::new(original.clone())).unwrap();
    let cases = [
        ("footer-magic", original.len() - 1),
        ("metadata", usize::try_from(footer.meta_offset).unwrap() + 2),
    ];
    for (name, offset) in cases {
        let mut corrupted = original.clone();
        corrupted[offset] ^= 0xFF;
        let bad = dir.path().join(format!("{name}.de"));
        std::fs::write(&bad, &corrupted).unwrap();
        assert_fail_closed(run_stub_exec(&bad), name);
    }
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
            let result = run_stub_exec(&thread_bin).expect("concurrent run failed");
            assert!(
                result.status.success() && String::from_utf8_lossy(&result.stdout).contains("hi"),
                "concurrent run {i} failed: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_no_panic(&result, "concurrent");
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

    let result = run_stub_exec(&bin).expect("valid fixture failed to run");
    assert!(
        result.status.success() && String::from_utf8_lossy(&result.stdout).contains("hi"),
        "valid fixture did not run: {}",
        String::from_utf8_lossy(&result.stderr)
    );

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
    assemble_fixture(&out, &payload, &meta);
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
    assemble_fixture(&out, &payload, &meta);
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
    assemble_fixture(&out, &payload, &meta);
    out
}

fn stub_bytes_for() -> Vec<u8> {
    if let Ok(path) = std::env::var("DAEDALUS_STUB_PATH") {
        return std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
    }
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
        assert_fail_closed(
            run_stub_result(&broken, "--daedalus-version"),
            &format!("truncated-{cut}"),
        );
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
        assert_fail_closed(
            run_stub_result(&broken, "--daedalus-version"),
            &format!("truncated-{cut}"),
        );
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
        assert_fail_closed(
            run_stub_result(&broken, "--daedalus-version"),
            &format!("truncated-{cut}"),
        );
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
    assemble_fixture(&out, &payload, &meta);

    let original = std::fs::read(&out).unwrap();
    for cut in [0usize, 1, original.len() / 2, original.len() - 1] {
        let broken = dir.path().join(format!("cut-{cut}.de"));
        std::fs::write(&broken, &original[..cut]).unwrap();
        assert_fail_closed(
            run_stub_result(&broken, "--daedalus-version"),
            &format!("truncated-{cut}"),
        );
    }
}
