//! SISR end-to-end integration tests against a mock HTTP update channel.
//!
//! The full reconstruction loop is exercised with the **real** launcher
//! binary (`CARGO_BIN_EXE_xbin-stub`, embedded into a freshly built `.xbin`):
//!
//! ```text
//! 1. build app_v1.xbin (SISR enabled)
//! 2. start a mock HTTP server serving the v1 → v2 delta (XBMR + chunks)
//! 3. run ./app_v1.xbin --xbin-update http://127.0.0.1:<port>
//! 4. assert app_v1.xbin became v2 and produces v2's output
//! ```

pub mod mock_server;
mod update_basic;
mod update_failures;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ed25519_dalek::SigningKey;
use sha2::{Digest, Sha256};
use xbin_core::assembly::assemble_xbin_with_sisr;
use xbin_core::format::Footer;
use xbin_core::sisr_header::read_sisr;
use xbin_core::sisr_stage::{build_artifacts, RemoteManifest, SisrBuildConfig};

use mock_server::MockHttpServer;

pub const KEY_SEED: [u8; 32] = [7u8; 32];
pub const CHUNK_TARGET: usize = 64 << 10;

/// The app script markers, same length so the tar layout only differs by the
/// payload bytes and chunk reuse is maximal.
pub const BODY_V1: &str = "echo v1-ok; exit 0";
pub const BODY_V2: &str = "echo v2-ok; exit 0";

pub fn key() -> SigningKey {
    SigningKey::from_bytes(&KEY_SEED)
}

pub fn meta() -> &'static [u8] {
    br#"{"name":"e2e-sisr","runtime":"bash","entrypoint":["/app/app.sh"],"payload_format":"zstd-tar","layers":[]}"#
}

/// Deterministic pseudo-random bytes shared by v1 and v2 (incompressible, so
/// the payload compresses to ~the same size and chunk reuse is stable).
pub fn random_buf(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

/// Rootfs tar: a shared incompressible blob followed by the app script. The
/// shared blob is byte-identical between versions, so only the trailing script
/// region changes.
pub fn rootfs_tar(body: &str, shared: &[u8]) -> Vec<u8> {
    let mut tar = tar::Builder::new(Vec::new());
    let mut blob = tar::Header::new_gnu();
    blob.set_size(shared.len() as u64);
    blob.set_mode(0o644);
    blob.set_mtime(1_600_000_000);
    blob.set_username("root").unwrap();
    blob.set_groupname("root").unwrap();
    blob.set_cksum();
    tar.append_data(&mut blob, "app/shared.bin", shared)
        .unwrap();
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

pub fn payload(body: &str, shared: &[u8]) -> Vec<u8> {
    zstd::encode_all(Cursor::new(rootfs_tar(body, shared)), 3).unwrap()
}

/// The footer content hash (`SHA-256(payload ‖ meta)`) — the version id.
pub fn payload_sha(body: &str, shared: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(payload(body, shared));
    h.update(meta());
    hex::encode(h.finalize())
}

pub fn footer_sha(path: &Path) -> String {
    let data = fs::read(path).unwrap();
    let footer = Footer::read_from(&mut Cursor::new(&data)).unwrap();
    footer.sha256_hex()
}

pub fn setup_trusted_keys(work: &Path) {
    let dir = work.join("trusted");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("update.key"), key().verifying_key().to_bytes()).unwrap();
}

pub fn build_xbin(work: &Path, stub: &Path, body: &str, shared: &[u8]) {
    let config = SisrBuildConfig {
        enabled: true,
        chunk_target_size: CHUNK_TARGET,
        signing_key: Some(key()),
    };
    let out = work.join("app.xbin");
    assemble_xbin_with_sisr(
        &out,
        &fs::read(stub).unwrap(),
        &payload(body, shared),
        meta(),
        false,
        false,
        None,
        &config,
    )
    .unwrap();
}

/// The delta artifacts to serve for an update of `current` to `body`.
pub struct StagedUpdate {
    pub remote: RemoteManifest,
    /// Sum of the bytes of the chunks *not* already present in `current`.
    pub changed_total: u64,
    /// Count of chunks the launcher must download.
    pub changed_count: usize,
    /// Route table for the mock server (`"/chunks/<hex>"` → bytes).
    pub chunks: HashMap<String, Vec<u8>>,
}

/// Stages v2 for the binary at `current`, signing with `signing_key`.
pub fn stage_update(
    current: &Path,
    body: &str,
    shared: &[u8],
    signing_key: SigningKey,
) -> StagedUpdate {
    let data = fs::read(current).unwrap();
    let have: HashSet<[u8; 32]> = read_sisr(&mut Cursor::new(&data))
        .unwrap()
        .expect("binary must embed a SISR manifest")
        .1
        .chunks
        .iter()
        .map(|c| c.hash)
        .collect();

    let payload = payload(body, shared);
    let artifacts = build_artifacts(
        &payload,
        &SisrBuildConfig {
            enabled: true,
            chunk_target_size: CHUNK_TARGET,
            signing_key: Some(signing_key),
        },
    )
    .unwrap();

    let mut chunks = HashMap::new();
    let mut changed_total = 0u64;
    let mut changed_count = 0usize;
    let mut pos = 0usize;
    for chunk in &artifacts.manifest.chunks {
        let end = pos + chunk.length as usize;
        if !have.contains(&chunk.hash) {
            changed_total += u64::from(chunk.length);
            changed_count += 1;
            chunks.insert(
                format!("/chunks/{}", hex::encode(chunk.hash)),
                payload[pos..end].to_vec(),
            );
        }
        pos = end;
    }

    StagedUpdate {
        remote: RemoteManifest {
            merkle_root: artifacts.merkle_root,
            signature: artifacts.signature,
            manifest: artifacts.manifest,
        },
        changed_total,
        changed_count,
        chunks,
    }
}

/// A running test environment: an isolated workdir, the built `.xbin`, a mock
/// update channel, and the staged v2 delta.
pub struct TestEnv {
    pub _tmp: tempfile::TempDir,
    pub work: PathBuf,
    pub app: PathBuf,
    pub v1_sha: String,
    pub server: MockHttpServer,
    pub base_url: String,
    pub staged: StagedUpdate,
}

pub fn env(v1: &str, v2: &str) -> TestEnv {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().to_path_buf();
    setup_trusted_keys(&work);
    let stub = PathBuf::from(env!("CARGO_BIN_EXE_xbin-stub"));
    let shared = random_buf(256 << 10, 42);

    build_xbin(&work, &stub, v1, &shared);
    let v1_sha = footer_sha(&work.join("app.xbin"));
    let staged = stage_update(&work.join("app.xbin"), v2, &shared, key());
    assert!(
        staged.changed_count > 0,
        "test delta must actually change something"
    );

    let (server, base_url) = MockHttpServer::start();
    server.route_manifest(&staged.remote.to_bytes());
    for (path, bytes) in &staged.chunks {
        server.route(path, bytes.clone());
    }

    let app = work.join("app.xbin");
    TestEnv {
        _tmp: tmp,
        work,
        app,
        v1_sha,
        server,
        base_url,
        staged,
    }
}

/// Runs `./app.xbin --xbin-update <base>` in the isolated environment.
pub fn run_update(env: &TestEnv) -> Output {
    Command::new(&env.app)
        .arg("--xbin-update")
        .arg(&env.base_url)
        .env("XBIN_TRUSTED_DIR", env.work.join("trusted"))
        .env("XDG_CACHE_HOME", env.work.join("cache"))
        .env("XDG_DATA_HOME", env.work.join("data"))
        .env("XBIN_HEALTH_TIMEOUT_MS", "3000")
        .output()
        .expect("failed to spawn xbin --xbin-update")
}

/// Runs the binary as a normal app in the isolated environment.
pub fn run_app(env: &TestEnv) -> Output {
    Command::new(&env.app)
        .env("XBIN_TRUSTED_DIR", env.work.join("trusted"))
        .env("XDG_CACHE_HOME", env.work.join("cache"))
        .env("XDG_DATA_HOME", env.work.join("data"))
        .env("XBIN_HEALTH_TIMEOUT_MS", "3000")
        .output()
        .expect("failed to spawn the xbin app")
}

/// Parsed reuse/fetch statistics from the launcher's stderr.
#[derive(Debug, Clone, Copy)]
pub struct Stats {
    pub reused_chunks: u64,
    pub fetched_chunks: u64,
    pub fetched_bytes: u64,
    pub total_chunks: u64,
}

/// Extracts the update statistics from the launcher's stderr line:
/// `[xbin] update applied: N chunks reused (…), M chunks fetched (…), total K`.
pub fn parse_stats(stderr: &str) -> Stats {
    let line = stderr
        .lines()
        .find(|l| l.contains("chunks fetched"))
        .unwrap_or_else(|| panic!("update stats line missing from: {stderr}"));
    // Number that immediately precedes `label` (e.g. `2 chunks reused`).
    let preceding = |label: &str| -> u64 {
        let pos = line
            .find(label)
            .unwrap_or_else(|| panic!("cannot find `{label}` in: {line}"));
        let before = &line[..pos];
        let bytes = before.as_bytes();
        let mut end = before.trim_end().len();
        while end > 0 && !bytes[end - 1].is_ascii_digit() {
            end -= 1;
        }
        let mut start = end;
        while start > 0 && bytes[start - 1].is_ascii_digit() {
            start -= 1;
        }
        before[start..end]
            .parse()
            .unwrap_or_else(|_| panic!("cannot parse count for `{label}` in: {line}"))
    };
    let total = line
        .split("total")
        .nth(1)
        .and_then(|rest| {
            rest.chars()
                .skip_while(|c| !c.is_ascii_digit())
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .ok()
        })
        .unwrap_or_else(|| panic!("cannot parse total in: {line}"));
    let fetched_bytes = line
        .split("chunks fetched (")
        .nth(1)
        .and_then(|rest| rest.split(')').next())
        .and_then(|b| {
            let (num, unit) = b.trim().split_once(' ')?;
            let (whole, frac) = num.split_once('.').unwrap_or((num, ""));
            let unit_bytes = match unit {
                "MiB" => 1024u64 * 1024,
                "KiB" => 1024,
                _ => 1,
            };
            let whole_bytes = whole.parse::<u64>().ok()?.checked_mul(unit_bytes)?;
            if frac.is_empty() {
                return Some(whole_bytes);
            }
            let len = u32::try_from(frac.len()).ok()?;
            let frac_bytes = frac.parse::<u64>().ok()?.checked_mul(unit_bytes)? / 10u64.pow(len);
            whole_bytes.checked_add(frac_bytes)
        })
        .unwrap_or_else(|| panic!("cannot parse fetched bytes in: {line}"));
    Stats {
        reused_chunks: preceding("chunks reused"),
        fetched_chunks: preceding("chunks fetched"),
        fetched_bytes,
        total_chunks: total,
    }
}
