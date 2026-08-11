//! E2E failure tests: every rejection path must leave the binary untouched
//! and exit non-zero. Each scenario serves a *signed-looking but invalid*
//! delta over the mock HTTP channel.

use ed25519_dalek::SigningKey;
use erebus_core::sisr_stage::sign;

use super::{env, footer_sha, run_update, BODY_V1, BODY_V2};

/// A manifest whose signature is valid but whose chunk table no longer commits
/// to the signed Merkle root (simulates a signer/editor bug).
fn staged_remote_with_tampered_chunk(
    e: &super::TestEnv,
) -> erebus_core::sisr_stage::RemoteManifest {
    let mut manifest = e.staged.remote.manifest.clone();
    manifest.chunks[0].hash[0] ^= 0xFF;
    let signature = sign(
        &manifest.serialize(),
        &e.staged.remote.merkle_root,
        &super::key(),
    );
    erebus_core::sisr_stage::RemoteManifest {
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
    e.server.route_manifest(&staged.remote.to_bytes());

    let out = run_update(&e);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "untrusted signature must fail");
    assert_refused(&e, &stderr, "signature verification failed");
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
    e.server.route_manifest(&bad.to_bytes());

    let out = run_update(&e);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "merkle mismatch must fail");
    assert_refused(&e, &stderr, "Merkle");
}
