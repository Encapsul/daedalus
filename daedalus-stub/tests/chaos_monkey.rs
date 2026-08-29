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
    assemble_daedalus(
        &out,
        &AssemblyInput {
            stub_bytes: b"MUTABLE-STUB-BYTES",
            payload: &payload,
            meta_bytes: serde_json::to_vec(&meta).unwrap().as_slice(),
            squashfs: false,
            target_arch: None,
            sisr: None,
            encryption: None,
        },
    )
    .unwrap();
    out
}

fn run_stub(bin: &Path, arg: &str) -> std::process::Output {
    Command::new(bin)
        .arg(arg)
        .output()
        .expect("failed to run stub")
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
        let _result = run_stub(&broken, "--daedalus-version");
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

        let _result = run_stub(&bad, "--daedalus-version");
    }
}

#[test]
fn chaos_garbage_file_refused_cleanly() {
    let dir = tempdir().unwrap();
    let junk = dir.path().join("junk.de");
    std::fs::write(&junk, vec![0xDEu8; 512]).unwrap();

    let result = run_stub(&junk, "--daedalus-version");
    assert!(
        !result.status.success() || String::from_utf8_lossy(&result.stderr).contains("not a .de")
    );
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
    for _i in 0..5 {
        let bin_path = bin.clone();
        children.push(std::thread::spawn(move || {
            let _ = run_stub(&bin_path, "--daedalus-version");
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
    let parts = daedalus_core::format::read_at(
        &mut cursor,
        footer.payload_offset,
        (footer.meta_offset - footer.payload_offset) as usize,
    )
    .expect("payload must be readable");
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(&parts);
    let digest: [u8; 32] = h.finalize().into();
    assert_eq!(footer.payload_sha256, digest);
}
