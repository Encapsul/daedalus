//! E2E happy-path tests: remote update, local staging, and the delta
//! bandwidth property (only the modified blocks + metadata overhead).

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use super::{env, footer_sha, parse_stats, payload_sha, run_app, run_update, BODY_V1, BODY_V2};

/// The full prompt-9 architecture loop: build v1, serve the delta over a mock
/// HTTP server, `--erebus-update`, then assert the binary became v2.
#[cfg(unix)]
#[test]
fn remote_update_swaps_in_v2_and_reports_reuse() {
    let e = env(BODY_V1, BODY_V2);

    let out = run_update(&e);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        out.status.success(),
        "--erebus-update must succeed: {stderr}"
    );
    assert!(stderr.contains("manifest verified"), "stderr: {stderr}");
    assert!(stderr.contains("chunks reused"), "stderr: {stderr}");
    assert!(stderr.contains("chunks fetched"), "stderr: {stderr}");
    assert!(stderr.contains("updated binary"), "stderr: {stderr}");

    let v2_sha = payload_sha(BODY_V2, &super::random_buf(256 << 10, 42));
    assert_eq!(
        footer_sha(&e.app),
        v2_sha,
        "binary must now be v2: {stderr}"
    );
    assert_ne!(e.v1_sha, v2_sha, "v1 and v2 must differ");
    assert!(
        stdout.is_empty(),
        "stdout is reserved for the app: {stdout}"
    );

    let mode = std::fs::metadata(&e.app).unwrap().permissions().mode();
    assert_ne!(
        mode & 0o111,
        0,
        "the swapped-in binary must keep its executable bit"
    );
}

/// Section 11 performance property: the reconstruction downloads at most the
/// modified chunks, plus a small metadata allowance.
#[test]
fn delta_download_is_bounded_by_modified_blocks() {
    let e = env(BODY_V1, BODY_V2);
    let out = run_update(&e);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "{stderr}");

    let stats = parse_stats(&stderr);
    assert_eq!(
        stats.fetched_chunks, e.staged.changed_count as u64,
        "the launcher must download exactly the changed chunks: {stderr}"
    );
    assert_eq!(
        stats.reused_chunks + stats.fetched_chunks,
        stats.total_chunks,
        "reused + fetched must cover the whole manifest: {stderr}"
    );
    let allowance = e.staged.changed_total + e.staged.changed_total / 50;
    assert!(
        stats.fetched_bytes <= allowance,
        "fetched {} B exceeds modified {} B + 2%: {stderr}",
        stats.fetched_bytes,
        e.staged.changed_total
    );
}

/// The updated binary, run normally, must produce the new version's output.
#[test]
fn updated_binary_runs_the_new_payload() {
    let e = env(BODY_V1, BODY_V2);
    let update = run_update(&e);
    assert!(
        update.status.success(),
        "{}",
        String::from_utf8_lossy(&update.stderr)
    );

    let app = run_app(&e);
    let stdout = String::from_utf8_lossy(&app.stdout).into_owned();
    assert!(app.status.success(), "app must exit 0 after update");
    assert!(
        stdout.contains("v2-ok"),
        "updated binary must produce v2 output, got: {stdout}"
    );
    assert!(
        !stdout.contains("v1-ok"),
        "v1 output must not appear after update: {stdout}"
    );
}

/// Non-regression: the local `$ERE_SISR_MANIFEST` staging path (mission 6)
/// still applies the delta and runs the new version.
#[test]
fn local_staging_path_still_applies() {
    let e = env(BODY_V1, BODY_V2);

    let manifest_path = e.work.join("update.manifest");
    let chunks_dir = e.work.join("chunks");
    std::fs::create_dir_all(&chunks_dir).unwrap();
    std::fs::write(&manifest_path, e.staged.remote.to_bytes()).unwrap();
    for (path, bytes) in &e.staged.chunks {
        std::fs::write(chunks_dir.join(path.trim_start_matches("/chunks/")), bytes).unwrap();
    }

    let out = std::process::Command::new(&e.app)
        .env("ERE_SISR_MANIFEST", &manifest_path)
        .env("ERE_TRUSTED_DIR", e.work.join("trusted"))
        .env("XDG_CACHE_HOME", e.work.join("cache"))
        .env("XDG_DATA_HOME", e.work.join("data"))
        .env("ERE_HEALTH_TIMEOUT_MS", "3000")
        .output()
        .expect("failed to spawn erebus with ERE_SISR_MANIFEST");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "local update must run v2: {stderr}");
    assert!(
        stdout.contains("v2-ok"),
        "local staging must produce v2 output: {stdout}"
    );
    assert_eq!(
        footer_sha(&e.app),
        payload_sha(BODY_V2, &super::random_buf(256 << 10, 42)),
        "local staging must swap the binary in place"
    );
}
