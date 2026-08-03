//! End-to-end health-gate and automatic-rollback tests for the launcher.
//!
//! These drive the real `xbin-stub` binary (embedded into a freshly built
//! `.xbin`) through the full SISR update flow and exercise the mission 8
//! guarantees:
//!
//! - a newly-updated version that crashes at startup is rolled back
//!   atomically and the previous version runs instead;
//! - a healthy update is confirmed, kept, and its snapshot discarded;
//! - a quarantined version is refused on re-install (anti-rollback loop).

use std::collections::HashSet;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ed25519_dalek::SigningKey;
use sha2::{Digest, Sha256};
use xbin_core::assembly::assemble_xbin_with_sisr;
use xbin_core::format::Footer;
use xbin_core::manifest::DeltaManifest;
use xbin_core::sisr::health::{HealthState, HealthStore};
use xbin_core::sisr_header::read_sisr;
use xbin_core::sisr_stage::{build_artifacts, RemoteManifest, SisrBuildConfig};

const KEY_SEED: [u8; 32] = [7u8; 32];
const CHUNK_TARGET: usize = 64 << 10;

fn key() -> SigningKey {
    SigningKey::from_bytes(&KEY_SEED)
}

fn meta() -> &'static [u8] {
    br#"{"name":"rollback-e2e","runtime":"bash","entrypoint":["/app/app.sh"],"payload_format":"zstd-tar","layers":[]}"#
}

/// Rootfs tar containing an executable `/app/app.sh` running `body`.
fn rootfs_tar(body: &str) -> Vec<u8> {
    let mut tar = tar::Builder::new(Vec::new());
    let mut app = tar::Header::new_gnu();
    app.set_size(body.len() as u64);
    app.set_mode(0o755);
    app.set_mtime(1_600_000_000);
    app.set_username("root").unwrap();
    app.set_groupname("root").unwrap();
    app.set_cksum();
    tar.append_data(&mut app, "app/app.sh", body.as_bytes())
        .unwrap();
    tar.into_inner().unwrap()
}

/// The zstd-compressed payload for a given app script body.
fn payload(body: &str) -> Vec<u8> {
    zstd::encode_all(Cursor::new(rootfs_tar(body)), 3).unwrap()
}

/// The content hash (`SHA-256(payload ‖ meta)`) of a version — its footer id.
fn payload_sha(body: &str) -> String {
    let mut h = Sha256::new();
    h.update(payload(body));
    h.update(meta());
    hex::encode(h.finalize())
}

fn footer_sha(path: &Path) -> String {
    let data = fs::read(path).unwrap();
    let footer = Footer::read_from(&mut Cursor::new(&data)).unwrap();
    footer.sha256_hex()
}

fn setup_trusted_keys(work: &Path) {
    let dir = work.join("trusted");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("update.key"), key().verifying_key().to_bytes()).unwrap();
}

fn build_xbin(work: &Path, stub: &Path, body: &str) {
    let config = SisrBuildConfig {
        enabled: true,
        chunk_target_size: CHUNK_TARGET,
        signing_key: Some(key()),
    };
    let out = work.join("app.xbin");
    assemble_xbin_with_sisr(
        &out,
        &fs::read(stub).unwrap(),
        &payload(body),
        meta(),
        false,
        false,
        None,
        &config,
    )
    .unwrap();
}

/// Stages an update to `body` for the binary at `current`. Returns the remote
/// manifest path and the target version's content hash.
fn stage_update(work: &Path, current: &Path, body: &str) -> (PathBuf, String) {
    let data = fs::read(current).unwrap();
    let have: HashSet<[u8; 32]> = read_sisr(&mut Cursor::new(&data))
        .unwrap()
        .expect("binary must embed a SISR manifest")
        .1
        .chunks
        .iter()
        .map(|c| c.hash)
        .collect();

    let target_payload = payload(body);
    let artifacts = build_artifacts(
        &target_payload,
        &SisrBuildConfig {
            enabled: true,
            chunk_target_size: CHUNK_TARGET,
            signing_key: Some(key()),
        },
    )
    .unwrap();

    let chunks_dir = work.join("chunks");
    fs::create_dir_all(&chunks_dir).unwrap();
    let mut pos = 0usize;
    for chunk in &artifacts.manifest.chunks {
        if !have.contains(&chunk.hash) {
            let end = pos + chunk.length as usize;
            fs::write(
                chunks_dir.join(hex::encode(chunk.hash)),
                &target_payload[pos..end],
            )
            .unwrap();
        }
        pos += chunk.length as usize;
    }

    let remote = RemoteManifest {
        merkle_root: artifacts.merkle_root,
        signature: artifacts.signature,
        manifest: DeltaManifest {
            version: artifacts.manifest.version,
            payload_len: artifacts.manifest.payload_len,
            chunks: artifacts.manifest.chunks,
        },
    };
    let manifest_path = work.join("update.manifest");
    fs::write(&manifest_path, remote.to_bytes()).unwrap();
    (manifest_path, payload_sha(body))
}

/// Runs the `.xbin` with the SISR manifest staged, in an isolated cache.
fn run_xbin(work: &Path, app: &Path, manifest: &Path) -> Output {
    Command::new(app)
        .env("XBIN_SISR_MANIFEST", manifest)
        .env("XBIN_TRUSTED_DIR", work.join("trusted"))
        .env("XDG_CACHE_HOME", work.join("cache"))
        .env("XDG_DATA_HOME", work.join("data"))
        .env("XBIN_HEALTH_MAX_ATTEMPTS", "1")
        .env("XBIN_HEALTH_TIMEOUT_MS", "3000")
        .output()
        .expect("failed to spawn the xbin launcher")
}

fn health_store(work: &Path) -> HealthStore {
    HealthStore::new(&work.join("cache").join("xbin").join("health"))
}

#[test]
fn crashing_update_is_rolled_back_and_old_version_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().to_path_buf();
    setup_trusted_keys(&work);
    let stub = PathBuf::from(env!("CARGO_BIN_EXE_xbin-stub"));

    build_xbin(&work, &stub, "echo v1-ok; exit 0");
    let v1_sha = footer_sha(&work.join("app.xbin"));
    let (manifest, v2_sha) = stage_update(&work, &work.join("app.xbin"), "echo v2-crash; exit 1");

    let out = run_xbin(&work, &work.join("app.xbin"), &manifest);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();

    assert!(
        out.status.success(),
        "rollback must end on the good version: {stderr}"
    );
    assert!(
        stdout.contains("v1-ok"),
        "old version must run after rollback: {stdout}"
    );
    assert!(stderr.contains("health gate"), "gate must run: {stderr}");
    assert_eq!(
        footer_sha(&work.join("app.xbin")),
        v1_sha,
        "on-disk binary must be restored to the previous version"
    );
    assert!(
        !work.join("app.xbin.bak").exists(),
        "snapshot must be discarded after restore"
    );
    assert_eq!(
        health_store(&work).load(&v2_sha).unwrap().unwrap().state,
        HealthState::Quarantined,
        "the failing version must be quarantined"
    );
}

#[test]
fn healthy_update_is_confirmed_and_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().to_path_buf();
    setup_trusted_keys(&work);
    let stub = PathBuf::from(env!("CARGO_BIN_EXE_xbin-stub"));

    build_xbin(&work, &stub, "echo v1; exit 0");
    let (manifest, v2_sha) = stage_update(&work, &work.join("app.xbin"), "echo v2-ok; exit 0");

    let out = run_xbin(&work, &work.join("app.xbin"), &manifest);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();

    assert!(
        out.status.success(),
        "healthy update must succeed: {stderr}"
    );
    assert!(stdout.contains("v2-ok"), "new version must run: {stdout}");
    assert_eq!(
        footer_sha(&work.join("app.xbin")),
        v2_sha,
        "healthy update must stay installed"
    );
    assert!(
        !work.join("app.xbin.bak").exists(),
        "snapshot must be discarded after confirmation"
    );
    assert_eq!(
        health_store(&work).load(&v2_sha).unwrap().unwrap().state,
        HealthState::Healthy
    );
}

#[test]
fn quarantined_version_is_refused_on_reinstall() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().to_path_buf();
    setup_trusted_keys(&work);
    let stub = PathBuf::from(env!("CARGO_BIN_EXE_xbin-stub"));

    build_xbin(&work, &stub, "echo v1-stable; exit 0");
    let v1_sha = footer_sha(&work.join("app.xbin"));
    let (manifest, v2_sha) = stage_update(&work, &work.join("app.xbin"), "echo v2-crash; exit 1");

    // First attempt: crashes, rolls back, quarantines v2.
    let first = run_xbin(&work, &work.join("app.xbin"), &manifest);
    assert!(
        first.status.success(),
        "first attempt must roll back cleanly: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(
        health_store(&work).load(&v2_sha).unwrap().unwrap().state,
        HealthState::Quarantined
    );

    // Re-attempt: the update engine must refuse the quarantined target.
    let second = run_xbin(&work, &work.join("app.xbin"), &manifest);
    let stderr = String::from_utf8_lossy(&second.stderr).into_owned();
    assert!(
        !second.status.success(),
        "re-installing a quarantined version must fail"
    );
    assert!(
        stderr.contains("quarantined"),
        "refusal must mention the quarantine: {stderr}"
    );
    assert_eq!(
        footer_sha(&work.join("app.xbin")),
        v1_sha,
        "binary must stay on the known-good version"
    );
}
