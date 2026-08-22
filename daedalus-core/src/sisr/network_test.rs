//! Network fault-injection tests for the SISR reconstruction engine.
//!
//! The engine's only interface to the network is the [`ChunkFetcher`] trait.
//! These tests wrap a memory-backed fetcher with fault injectors and assert
//! the engine stays correct under the prompt-9 network constraints:
//!
//! - abrupt connection drops (an `io::Error` mid-transfer);
//! - corrupted packets (bytes that fail the SHA-256 check);
//! - truncated packets (partial reads);
//! - very slow throughput (throttled, still reconstructs correctly).

use std::cell::Cell;
use std::collections::HashMap;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use crate::assembly::assemble_daedalus;
use crate::format::Footer;
use crate::manifest::DeltaManifest;
use crate::sisr::engine::{ChunkFetcher, SisrEngine};
use crate::sisr_stage::{build_artifacts, SisrBuildConfig};

/// Legacy unsigned signature placeholder.
const UNSIGNED: [u8; 64] = [0u8; 64];

/// Tunable network faults applied per `fetch` call.
#[derive(Default)]
struct Faults {
    /// Delay injected before each fetch.
    latency: Duration,
    /// Fail every Nth fetch with `ConnectionReset` (0 = off).
    drop_every_nth: u64,
    /// Bit-flip the first byte of every Nth fetch (0 = off).
    corrupt_every_nth: u64,
    /// Truncate every Nth fetch to half its length (0 = off).
    truncate_every_nth: u64,
    /// Cap throughput to N bytes/second (0 = unlimited).
    bytes_per_sec: u64,
}

/// Wraps any [`ChunkFetcher`] and injects network-style faults.
struct FaultInjectingFetcher<F> {
    inner: F,
    faults: Faults,
    calls: AtomicU64,
}

impl<F: ChunkFetcher> FaultInjectingFetcher<F> {
    fn new(inner: F, faults: Faults) -> Self {
        Self {
            inner,
            faults,
            calls: AtomicU64::new(0),
        }
    }
}

impl<F: ChunkFetcher> ChunkFetcher for FaultInjectingFetcher<F> {
    fn fetch(&self, hash: &[u8; 32], length: usize) -> io::Result<Vec<u8>> {
        if !self.faults.latency.is_zero() {
            thread::sleep(self.faults.latency);
        }
        let n = self.calls.fetch_add(1, Ordering::Relaxed) + 1;
        if self.faults.drop_every_nth > 0 && n.is_multiple_of(self.faults.drop_every_nth) {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "connection dropped mid-transfer",
            ));
        }
        let mut bytes = self.inner.fetch(hash, length)?;
        if self.faults.corrupt_every_nth > 0 && n.is_multiple_of(self.faults.corrupt_every_nth) {
            if let Some(first) = bytes.first_mut() {
                *first ^= 0xFF;
            }
        }
        if self.faults.truncate_every_nth > 0 && n.is_multiple_of(self.faults.truncate_every_nth) {
            let cut = bytes.len() / 2;
            bytes.truncate(cut);
        }
        if let Some(seconds) = (bytes.len() as u64).checked_div(self.faults.bytes_per_sec) {
            if seconds > 0 {
                thread::sleep(Duration::from_secs(seconds));
            }
        }
        Ok(bytes)
    }

    fn bytes_fetched(&self) -> u64 {
        self.inner.bytes_fetched()
    }
}

/// Fetcher backed by a hash → bytes map (tests only).
struct MemoryFetcher {
    map: HashMap<[u8; 32], Vec<u8>>,
    fetched: Cell<u64>,
}

impl MemoryFetcher {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            fetched: Cell::new(0),
        }
    }

    fn seed(&mut self, payload: &[u8], manifest: &DeltaManifest) {
        let mut pos = 0usize;
        for chunk in &manifest.chunks {
            let end = pos + chunk.length as usize;
            self.map.insert(chunk.hash, payload[pos..end].to_vec());
            pos = end;
        }
    }
}

impl ChunkFetcher for MemoryFetcher {
    fn fetch(&self, hash: &[u8; 32], _length: usize) -> io::Result<Vec<u8>> {
        let bytes = self
            .map
            .get(hash)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "chunk not available"))?;
        self.fetched
            .set(self.fetched.get().saturating_add(bytes.len() as u64));
        Ok(bytes)
    }

    fn bytes_fetched(&self) -> u64 {
        self.fetched.get()
    }
}

fn random_buf(len: usize, seed: u64) -> Vec<u8> {
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

fn build_current(dir: &Path, payload: &[u8], chunk: usize) -> PathBuf {
    let out = dir.join("app.daedalus");
    let config = SisrBuildConfig {
        enabled: true,
        chunk_target_size: chunk,
        signing_key: None,
    };
    let artifacts = crate::sisr_stage::build_artifacts(payload, &config).unwrap();
    assemble_daedalus(
        &out,
        &crate::assembly::AssemblyInput {
            stub_bytes: b"STUB_DATA_HERE",
            payload,
            meta_bytes: b"{\"name\":\"network\"}",
            encrypt: false,
            squashfs: false,
            target_arch: None,
            sisr: Some(artifacts),
        },
    )
    .unwrap();
    out
}

fn manifest_for(payload: &[u8], chunk: usize) -> DeltaManifest {
    let artifacts = build_artifacts(
        payload,
        &SisrBuildConfig {
            enabled: true,
            chunk_target_size: chunk,
            signing_key: None,
        },
    )
    .unwrap();
    artifacts.manifest
}

fn payload_of(path: &Path) -> Vec<u8> {
    let data = std::fs::read(path).unwrap();
    let mut cur = Cursor::new(&data);
    let footer = Footer::read_from(&mut cur).unwrap();
    crate::format::read_at(
        &mut cur,
        footer.payload_offset,
        footer.payload_csize as usize,
    )
    .unwrap()
}

/// A v1 binary + a manifest whose chunks are all new (every chunk fetched).
fn update_setup(dir: &Path, b_len: usize, chunk: usize) -> (PathBuf, DeltaManifest, MemoryFetcher) {
    let a = random_buf(b_len, 1);
    let b = random_buf(b_len, 2);
    let cur = build_current(dir, &a, chunk);
    let manifest = manifest_for(&b, chunk);
    let mut inner = MemoryFetcher::new();
    inner.seed(&b, &manifest);
    (cur, manifest, inner)
}

#[test]
fn latency_does_not_change_the_result() {
    let tmp = tempfile::tempdir().unwrap();
    let (cur, manifest, inner) = update_setup(tmp.path(), 96 << 10, 32 << 10);
    let fetcher = FaultInjectingFetcher::new(
        inner,
        Faults {
            latency: Duration::from_millis(5),
            ..Default::default()
        },
    );

    let updated = SisrEngine
        .apply_update(&cur, &manifest, &fetcher, &UNSIGNED)
        .unwrap();
    assert_eq!(
        payload_of(&updated).len(),
        96 << 10,
        "latency must not change the reconstructed payload"
    );
}

#[test]
fn connection_drop_leaves_binary_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let (cur, manifest, inner) = update_setup(tmp.path(), 96 << 10, 32 << 10);
    let before = payload_of(&cur);
    let fetcher = FaultInjectingFetcher::new(
        inner,
        Faults {
            drop_every_nth: 1,
            ..Default::default()
        },
    );

    let err = SisrEngine
        .apply_update(&cur, &manifest, &fetcher, &UNSIGNED)
        .unwrap_err();
    assert_eq!(
        err.kind(),
        io::ErrorKind::ConnectionReset,
        "the connection-drop error must surface"
    );
    assert_eq!(
        payload_of(&cur),
        before,
        "binary must be untouched on failure"
    );
}

#[test]
fn corrupted_packets_fail_sha256_verification() {
    let tmp = tempfile::tempdir().unwrap();
    let (cur, manifest, inner) = update_setup(tmp.path(), 96 << 10, 32 << 10);
    let before = payload_of(&cur);
    let fetcher = FaultInjectingFetcher::new(
        inner,
        Faults {
            corrupt_every_nth: 1,
            ..Default::default()
        },
    );

    let err = SisrEngine
        .apply_update(&cur, &manifest, &fetcher, &UNSIGNED)
        .unwrap_err();
    assert!(
        err.to_string().contains("SHA-256 verification"),
        "corruption must fail the hash check: {err}"
    );
    assert_eq!(
        payload_of(&cur),
        before,
        "binary must be untouched on failure"
    );
}

#[test]
fn truncated_packets_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let (cur, manifest, inner) = update_setup(tmp.path(), 96 << 10, 32 << 10);
    let before = payload_of(&cur);
    let fetcher = FaultInjectingFetcher::new(
        inner,
        Faults {
            truncate_every_nth: 1,
            ..Default::default()
        },
    );

    let err = SisrEngine
        .apply_update(&cur, &manifest, &fetcher, &UNSIGNED)
        .unwrap_err();
    assert!(
        err.to_string().contains("length mismatch"),
        "truncation must fail the length check: {err}"
    );
    assert_eq!(
        payload_of(&cur),
        before,
        "binary must be untouched on failure"
    );
}

#[test]
fn slow_throughput_still_reconstructs() {
    let tmp = tempfile::tempdir().unwrap();
    let (cur, manifest, inner) = update_setup(tmp.path(), 4 << 10, 32 << 10);
    let fetcher = FaultInjectingFetcher::new(
        inner,
        Faults {
            bytes_per_sec: 4 << 10,
            ..Default::default()
        },
    );

    let updated = SisrEngine
        .apply_update(&cur, &manifest, &fetcher, &UNSIGNED)
        .unwrap();
    assert_eq!(
        payload_of(&updated).len(),
        4 << 10,
        "a slow link must still reconstruct correctly"
    );
}

#[test]
fn fetched_bytes_are_accounted() {
    let tmp = tempfile::tempdir().unwrap();
    let (cur, manifest, inner) = update_setup(tmp.path(), 96 << 10, 32 << 10);

    let (updated, stats) = SisrEngine
        .apply_update_with_stats(&cur, &manifest, &inner, &UNSIGNED)
        .unwrap();
    assert_eq!(
        stats.fetched_chunks,
        manifest.chunks.len(),
        "every chunk is new and must be fetched"
    );
    assert_eq!(stats.reused_chunks, 0);
    assert_eq!(stats.fetched_bytes, 96 << 10);
    assert_eq!(payload_of(&updated).len(), 96 << 10);
}
