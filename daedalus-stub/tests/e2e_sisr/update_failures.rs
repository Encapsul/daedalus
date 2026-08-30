#![allow(missing_docs)]
//! E2E failure tests: every rejection path must leave the binary untouched
//! and exit non-zero. Each scenario serves a *signed-looking but invalid*
//! delta over the mock HTTP channel.

use daedalus_core::sisr_stage::sign;
use ed25519_dalek::SigningKey;
use std::process::Command;

use super::{env, footer_sha, run_update, BODY_V1, BODY_V2};

/// A manifest whose signature is valid but whose chunk table no longer commits
/// to the signed Merkle root (simulates a signer/editor bug).
fn staged_remote_with_tampered_chunk(
    e: &super::TestEnv,
) -> daedalus_core::sisr_stage::RemoteManifest {
    let mut manifest = e.staged.remote.manifest.clone();
    manifest.chunks[0].hash[0] ^= 0xFF;
    let signature = sign(
        &manifest.serialize().unwrap(),
        &e.staged.remote.merkle_root,
        &super::key(),
    );
    daedalus_core::sisr_stage::RemoteManifest {
        merkle_root: e.staged.remote.merkle_root,
        signature,
        manifest,
    }
}

fn assert_refused(e: &super::TestEnv, stderr: &str, needle: &str) {
    assert!(
        stderr.contains(needle),
        "stderr must mention `{needle}`: {stderr}"
    );
    assert_eq!(
        footer_sha(&e.app),
        e.v1_sha,
        "a refused update must leave the binary untouched: {stderr}"
    );
    assert!(
        std::fs::metadata(format!("{}.bak", e.app.display())).is_err(),
        "no snapshot may remain after a refused update"
    );
}

/// The update must refuse a manifest signed by a key outside the trusted set.
#[test]
fn untrusted_signature_is_refused() {
    let e = env(BODY_V1, BODY_V2);
    let rogue = SigningKey::from_bytes(&[9u8; 32]);
    let staged = super::stage_update(&e.app, BODY_V2, &super::random_buf(256 << 10, 42), rogue);
    e.server.route_manifest(&staged.remote.to_bytes().unwrap());

    let out = run_update(&e);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "untrusted signature must fail");
    assert_refused(&e, &stderr, "signature verification failed");
}

/// An unsigned remote manifest (all-zeros signature — what a build without
/// `--key` produces) must be refused before any write: the update channel
/// cannot introduce a binary that would fail at-rest verification.
#[test]
fn unsigned_remote_manifest_is_refused() {
    let e = env(BODY_V1, BODY_V2);
    let unsigned = daedalus_core::sisr_stage::RemoteManifest {
        merkle_root: e.staged.remote.merkle_root,
        signature: [0u8; 64],
        manifest: e.staged.remote.manifest.clone(),
    };
    e.server.route_manifest(&unsigned.to_bytes().unwrap());

    let out = run_update(&e);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "unsigned manifest must fail");
    assert_refused(&e, &stderr, "signature");
}

/// Garbage at `/manifest` must be rejected before any write.
#[test]
fn corrupt_manifest_is_refused() {
    let e = env(BODY_V1, BODY_V2);
    e.server.route_manifest(b"\x00\x01\x02broken");

    let out = run_update(&e);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "corrupt manifest must fail");
    assert_refused(&e, &stderr, "manifest");
}

/// A chunk that 404s (missing on the server) must abort the update.
#[test]
fn missing_chunk_is_refused() {
    let e = env(BODY_V1, BODY_V2);
    let dropped = e.staged.chunks.keys().next().unwrap().clone();
    e.server.route(&dropped, Vec::new());

    let out = run_update(&e);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "missing chunk must fail: {stderr}");
    assert_refused(&e, &stderr, "chunk");
}

/// A chunk shorter than its manifest entry must be rejected on the spot.
#[test]
fn truncated_chunk_is_refused() {
    let e = env(BODY_V1, BODY_V2);
    let (path, bytes) = e.staged.chunks.iter().next().unwrap();
    e.server.route(path, bytes[..bytes.len() / 2].to_vec());

    let out = run_update(&e);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "truncated chunk must fail: {stderr}");
    assert_refused(&e, &stderr, "chunk");
}

/// A chunk whose bytes were corrupted in transit fails the SHA-256 check.
#[test]
fn corrupted_chunk_bytes_are_rejected() {
    let e = env(BODY_V1, BODY_V2);
    let (path, bytes) = e.staged.chunks.iter().next().unwrap();
    let mut bad = bytes.clone();
    let mid = bad.len() / 2;
    bad[mid] ^= 0xFF;
    e.server.route(path, bad);

    let out = run_update(&e);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "corrupted chunk must fail: {stderr}");
    assert_refused(&e, &stderr, "SHA-256");
}

/// A manifest whose chunk table disagrees with its signed Merkle root must be
/// refused before any chunk is fetched.
#[test]
fn merkle_root_mismatch_is_refused() {
    let e = env(BODY_V1, BODY_V2);
    let bad = staged_remote_with_tampered_chunk(&e);
    e.server.route_manifest(&bad.to_bytes().unwrap());

    let out = run_update(&e);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "merkle mismatch must fail");
    assert_refused(&e, &stderr, "Merkle");
}

/// An unsigned SISR binary (built without `--key`) must be refused at cold
/// start — at-rest authenticity is mandatory unless explicitly waived.
#[test]
fn unsigned_sisr_binary_is_refused_at_cold_start() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path();
    super::setup_trusted_keys(work);
    let stub = std::path::PathBuf::from(env!("CARGO_BIN_EXE_daedalus-stub"));
    let shared = super::random_buf(256 << 10, 42);
    super::build_daedalus_unsigned(work, &stub, BODY_V1, &shared);
    let app = work.join("app.daedalus");

    // Default: fail closed.
    let out = Command::new(&app)
        .env("DAEDALUS_TRUSTED_DIR", work.join("trusted"))
        .env("XDG_CACHE_HOME", work.join("cache"))
        .env("XDG_DATA_HOME", work.join("data"))
        .output()
        .expect("failed to spawn the unsigned daedalus app");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        !out.status.success(),
        "unsigned SISR must be refused: {stderr}"
    );
    assert!(stderr.contains("unsigned"), "stderr must say why: {stderr}");

    // Explicit waiver: the legacy escape hatch lets it run.
    let out = Command::new(&app)
        .env("DAEDALUS_TRUSTED_DIR", work.join("trusted"))
        .env("XDG_CACHE_HOME", work.join("cache"))
        .env("XDG_DATA_HOME", work.join("data"))
        .env("DAEDALUS_SISR_ALLOW_UNSIGNED", "1")
        .output()
        .expect("failed to spawn the waived daedalus app");
    assert!(
        out.status.success(),
        "waived run must succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
