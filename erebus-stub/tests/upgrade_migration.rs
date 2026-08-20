//! Cross-version migration tests: legacy (v1, SISR-less) binaries must run on
//! the current v2 launcher unchanged, and `upgrade_binary` must promote them
//! to SISR-enabled binaries that gain delta auto-update — without touching a
//! single payload byte.

#[path = "e2e_sisr/mod.rs"]
mod e2e_sisr;

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;

use erebus_core::assembly::{assemble_erebus, AssemblyInput};
use erebus_core::format::Footer;
use erebus_core::legacy::upgrade_binary;
use erebus_core::sisr_stage::SisrBuildConfig;

use e2e_sisr::mock_server::MockHttpServer;
use e2e_sisr::*;

fn isolated_env(work: &Path) -> Vec<(&str, PathBuf)> {
    vec![
        ("ERE_TRUSTED_DIR", work.join("trusted")),
        ("XDG_CACHE_HOME", work.join("cache")),
        ("XDG_DATA_HOME", work.join("data")),
    ]
}

/// Builds a genuine legacy file: SISR stage disabled, current stub embedded.
fn build_legacy(work: &Path, stub: &Path, body: &str, shared: &[u8]) -> PathBuf {
    let out = work.join("legacy.erebus");
    let stub_bytes = fs::read(stub).unwrap();
    assemble_erebus(
        &out,
        &AssemblyInput {
            stub_bytes: &stub_bytes,
            payload: &payload(body, shared),
            meta_bytes: meta(),
            encrypt: false,
            squashfs: false,
            target_arch: None,
            sisr: None,
        },
    )
    .unwrap();
    out
}

/// Invariant (prompt §5): a v1 binary is read, extracted, and executed by the
/// v2 runtime without modification and without any superfluous warning.
#[test]
fn legacy_binary_runs_on_v2_runtime() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().to_path_buf();
    setup_trusted_keys(&work);
    let stub = PathBuf::from(env!("CARGO_BIN_EXE_erebus-stub"));
    let shared = random_buf(256 << 10, 42);
    let app = build_legacy(&work, &stub, BODY_V1, &shared);

    let out = Command::new(&app)
        .envs(isolated_env(&work))
        .env("ERE_HEALTH_TIMEOUT_MS", "3000")
        .output()
        .expect("failed to spawn legacy binary");

    assert!(out.status.success(), "legacy binary must run: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stdout.contains("v1-ok"),
        "legacy payload must execute: {stdout}"
    );
    assert!(
        !stderr.contains("[erebus]"),
        "legacy run must produce no launcher warnings: {stderr}"
    );
}

/// The migration value: after `upgrade_binary`, the same binary gains SISR
/// auto-update and applies a v2 delta through the update channel.
#[test]
fn upgraded_binary_gains_auto_update() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().to_path_buf();
    setup_trusted_keys(&work);
    let stub = PathBuf::from(env!("CARGO_BIN_EXE_erebus-stub"));
    let shared = random_buf(256 << 10, 42);

    let legacy = build_legacy(&work, &stub, BODY_V1, &shared);
    let upgraded = work.join("upgraded.erebus");
    let config = SisrBuildConfig {
        enabled: true,
        chunk_target_size: CHUNK_TARGET,
        signing_key: Some(key()),
    };
    upgrade_binary(&legacy, &upgraded, &config).unwrap();

    // Stage the v2 delta against the upgraded binary and serve it.
    let staged = stage_update(&upgraded, BODY_V2, &shared, key());
    assert!(staged.changed_count > 0);
    let (server, base_url) = MockHttpServer::start();
    server.route_manifest(&staged.remote.to_bytes());
    for (path, bytes) in &staged.chunks {
        server.route(path, bytes.clone());
    }

    let out = Command::new(&upgraded)
        .arg(format!("--erebus-update={base_url}"))
        .envs(isolated_env(&work))
        .env("ERE_HEALTH_TIMEOUT_MS", "3000")
        .output()
        .expect("failed to spawn upgraded binary --erebus-update");
    assert!(
        out.status.success(),
        "upgraded binary must accept the delta: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    let stats = parse_stats(&stderr);
    assert!(
        stats.fetched_chunks > 0,
        "delta must fetch new chunks: {stderr}"
    );
    assert_eq!(
        footer_sha(&upgraded),
        payload_sha(BODY_V2, &shared),
        "binary must now be v2"
    );

    // The updated binary runs the new payload.
    let run = Command::new(&upgraded)
        .envs(isolated_env(&work))
        .env("ERE_HEALTH_TIMEOUT_MS", "3000")
        .output()
        .expect("failed to spawn updated binary");
    assert!(run.status.success());
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(stdout.contains("v2-ok"), "v2 payload must run: {stdout}");
}

/// Security (prompt §10): the conversion copies the stored payload through
/// byte-for-byte, so payload-internal checksums (e.g. `SquashFS`) are preserved.
#[test]
fn upgraded_binary_preserves_payload_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().to_path_buf();
    let stub = PathBuf::from(env!("CARGO_BIN_EXE_erebus-stub"));
    let shared = random_buf(256 << 10, 42);

    let legacy = build_legacy(&work, &stub, BODY_V1, &shared);
    let before = fs::read(&legacy).unwrap();
    let in_footer = Footer::read_from(&mut Cursor::new(&before)).unwrap();

    let upgraded = work.join("upgraded.erebus");
    let config = SisrBuildConfig {
        enabled: true,
        chunk_target_size: CHUNK_TARGET,
        signing_key: Some(key()),
    };
    upgrade_binary(&legacy, &upgraded, &config).unwrap();
    let after = fs::read(&upgraded).unwrap();

    let payload_end = (in_footer.payload_offset + in_footer.payload_csize) as usize;
    assert_eq!(
        &after[..payload_end],
        &before[..payload_end],
        "stub and payload must be copied byte-for-byte"
    );
    let out_footer = Footer::read_from(&mut Cursor::new(&after)).unwrap();
    assert_eq!(
        out_footer.payload_sha256, in_footer.payload_sha256,
        "the integrity hash must be unchanged"
    );
}
