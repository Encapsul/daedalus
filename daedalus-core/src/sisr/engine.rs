//! `SISR` runtime engine — incremental, local reconstruction of a `.daedalus`.
//!
//! The engine rebuilds the executable on disk from the current binary plus a
//! delta manifest: unchanged chunks are copied out of the running file
//! (`self`), missing chunks come from a [`ChunkFetcher`], every chunk is
//! hash-verified before it is written, and the whole file is swapped in
//! atomically. See `docs/src/architecture/runtime-launcher.md`.

use std::cell::Cell;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::cas::ObjectStore;
use crate::format::{self, Footer};
use crate::manifest::DeltaManifest;
use crate::sisr::swap::AtomicWriter;
use crate::sisr_header::{read_sisr, SisrFooterExt, SISR_VERSION};
use crate::sisr_stage::sha256_digest as sha256_of;
use crate::sisr_stage::{ct_eq_hash, merkle_root_of};

/// Supplies raw chunk bytes addressed by their SHA-256 content hash.
pub trait ChunkFetcher {
    /// Returns the bytes that must SHA-256 to `hash` (the engine enforces it).
    fn fetch(&self, hash: &[u8; 32], length: usize) -> io::Result<Vec<u8>>;

    /// Total bytes returned by `fetch` so far (network/bandwidth accounting).
    fn bytes_fetched(&self) -> u64;
}

/// [`ChunkFetcher`] serving chunk files from a local directory keyed by hex
/// content hash (`<root>/<64-hex-hash>`).
///
/// Internally delegates to `DiskObjectStore` (from `cas.rs`) so that Sisr chunk
/// caching and the Phase 4 layer registry share one CAS on disk — eliminating
/// the ad-hoc directory layout that previously existed here.
pub struct DirectoryChunkFetcher {
    store: crate::cas::DiskObjectStore,
    fetched: Cell<u64>,
}

impl DirectoryChunkFetcher {
    pub fn new(root: &Path) -> Self {
        let store = crate::cas::DiskObjectStore::new(root)
            .expect("cas dir creation should not fail for a writable temp dir");
        Self {
            store,
            fetched: Cell::new(0),
        }
    }
}

impl ChunkFetcher for DirectoryChunkFetcher {
    fn fetch(&self, hash: &[u8; 32], _length: usize) -> io::Result<Vec<u8>> {
        let bytes = self
            .store
            .get(hash)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "chunk not found"))?;
        let total = self
            .fetched
            .get()
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "fetch counter overflow"))?;
        self.fetched.set(total);
        Ok(bytes)
    }

    fn bytes_fetched(&self) -> u64 {
        self.fetched.get()
    }
}

/// Statistics for one reconstruction run.
#[derive(Debug, Clone, Copy, Default)]
pub struct SisrUpdateStats {
    pub reused_chunks: usize,
    pub fetched_chunks: usize,
    pub reused_bytes: u64,
    pub fetched_bytes: u64,
}

/// In-memory reconstruction engine (stateless, safe to call from anywhere).
pub struct SisrEngine;

impl SisrEngine {
    /// Rebuilds `current_bin` against `manifest` and swaps it in atomically.
    ///
    /// The caller is responsible for authenticating `manifest` (e.g. via a
    /// signature-checked [`RemoteManifest`]) and for passing the verified
    /// manifest signature as `manifest_sig`; this engine enforces the
    /// cryptographic part it can: every chunk — reused from `self` or fetched
    /// — must SHA-256 to its entry in the manifest before it is written.
    ///
    /// Returns the canonical path of the updated binary.
    ///
    /// [`RemoteManifest`]: crate::sisr_stage::RemoteManifest
    pub fn apply_update(
        &self,
        current_bin: &Path,
        manifest: &DeltaManifest,
        fetcher: &dyn ChunkFetcher,
        manifest_sig: &[u8; 64],
    ) -> io::Result<PathBuf> {
        Ok(self
            .apply_update_with_stats(current_bin, manifest, fetcher, manifest_sig)?
            .0)
    }

    /// SHA-256(`payload` ‖ `meta_bytes`) of the binary that [`apply_update`]
    /// would produce — without writing anything.
    ///
    /// The launcher uses this *before* a swap to refuse re-installing a
    /// version the health gate already quarantined. Chunks are resolved with
    /// the same reuse-then-fetch policy as the real update, so the returned
    /// hash is exactly the `payload_sha256` the rebuilt footer would carry.
    ///
    /// [`apply_update`]: Self::apply_update
    pub fn target_payload_sha256(
        &self,
        current_bin: &Path,
        manifest: &DeltaManifest,
        fetcher: &dyn ChunkFetcher,
    ) -> io::Result<[u8; 32]> {
        let current = fs::canonicalize(current_bin)?;
        let mut exe = File::open(&current)?;
        let footer = Footer::read_from(&mut exe)?;
        let meta_bytes = read_at(
            &mut exe,
            footer.meta_offset,
            usize::try_from(footer.meta_size).map_err(|_| err("meta size overflow"))?,
        )?;
        let index = build_reuse_index(&mut exe, &footer)?;
        checked_payload_len(manifest)?;

        let mut hasher = Sha256::new();
        for chunk in &manifest.chunks {
            let (bytes, _) = resolve_chunk_bytes(&mut exe, &index, chunk, fetcher)?;
            hasher.update(&bytes);
        }
        hasher.update(&meta_bytes);
        Ok(hasher.finalize().into())
    }

    /// Like [`Self::apply_update`], but also returns reuse/fetch statistics
    /// (used to verify the bandwidth-reduction property in tests).
    ///
    /// # Authenticity at rest (roadmap #45)
    ///
    /// `manifest_sig` must be the Ed25519 signature over
    /// `merkle_root ‖ manifest_bytes` from the [`RemoteManifest`] the caller
    /// just verified against the trusted keys. The engine has no signing key
    /// (keys live at the build site), so the publisher pre-signs every
    /// release manifest and the update carries that signature into the
    /// rebuilt binary's SISR extension — the same field a fresh build embeds.
    /// The launcher then re-verifies it offline on every cold start, so an
    /// updated binary keeps its at-rest authenticity instead of degrading to
    /// integrity-only. Passing all-zeros yields a binary the launcher refuses
    /// to run unless `DAEDALUS_SISR_ALLOW_UNSIGNED` is set.
    ///
    /// [`RemoteManifest`]: crate::sisr_stage::RemoteManifest
    pub fn apply_update_with_stats(
        &self,
        current_bin: &Path,
        manifest: &DeltaManifest,
        fetcher: &dyn ChunkFetcher,
        manifest_sig: &[u8; 64],
    ) -> io::Result<(PathBuf, SisrUpdateStats)> {
        let current = fs::canonicalize(current_bin)?;
        let mut exe = File::open(&current)?;
        let footer = Footer::read_from(&mut exe)?;
        let meta_bytes = read_at(
            &mut exe,
            footer.meta_offset,
            usize::try_from(footer.meta_size).map_err(|_| err("meta size overflow"))?,
        )?;
        let index = build_reuse_index(&mut exe, &footer)?;
        let target_len = checked_payload_len(manifest)?;
        let stub_len =
            usize::try_from(footer.payload_offset).map_err(|_| err("payload offset overflow"))?;

        let parent = current
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no parent directory"))?;
        let mut w = AtomicWriter::new(parent, "app.daedalus.sisr")?;
        w.file_mut().write_all(&read_at(&mut exe, 0, stub_len)?)?;

        let mut hasher = Sha256::new();
        let mut stats = SisrUpdateStats::default();
        for chunk in &manifest.chunks {
            write_chunk(
                &mut exe,
                &index,
                chunk,
                fetcher,
                w.file_mut(),
                &mut hasher,
                &mut stats,
            )?;
        }
        hasher.update(&meta_bytes);
        let payload_sha256: [u8; 32] = hasher.finalize().into();
        w.file_mut().write_all(&meta_bytes)?;

        let manifest_bytes = manifest.serialize()?;
        let manifest_offset = footer
            .payload_offset
            .checked_add(target_len)
            .and_then(|p| p.checked_add(footer.meta_size))
            .ok_or_else(|| err("manifest offset overflow"))?;
        let ext = SisrFooterExt {
            sisr_version: SISR_VERSION,
            chunk_table_offset: manifest_offset,
            chunk_table_len: u32::try_from(manifest_bytes.len())
                .map_err(|_| err("manifest exceeds capacity"))?,
            merkle_root: merkle_root_of(manifest),
            // The publisher-signed link: identical message format
            // (`merkle_root ‖ manifest_bytes`) to the remote manifest the
            // caller verified, so the rebuilt binary stays offline-verifiable.
            signature: *manifest_sig,
        };
        w.file_mut().write_all(&manifest_bytes)?;
        w.file_mut().write_all(&ext.pack())?;

        let new_footer = Footer {
            format_version: footer.format_version,
            arch: footer.arch,
            flags: (footer.flags & !format::FLAG_SIGNED) | format::FLAG_SISR,
            payload_offset: footer.payload_offset,
            payload_csize: target_len,
            payload_usize: footer.payload_usize,
            payload_sha256,
            meta_offset: footer
                .payload_offset
                .checked_add(target_len)
                .ok_or_else(|| err("meta offset overflow"))?,
            meta_size: footer.meta_size,
            sig_offset: 0,
        };
        write_footer(w.file_mut(), &new_footer)?;

        // Preserve the source file's mode: `File::create` yields 0o644, so a
        // plain rename would strip the executable bit from a replaced binary.
        let perms = fs::metadata(&current)?.permissions();
        w.file_mut().set_permissions(perms)?;

        w.commit(&current)?;
        Ok((current, stats))
    }
}

/// Absolute chunk offsets in the *current* payload, derived from its own
/// embedded manifest. `read_sisr` failing (pre-SISR or inconsistent binary)
/// yields an empty index: correctness never depends on it, only reuse does.
fn build_reuse_index(
    exe: &mut File,
    footer: &Footer,
) -> io::Result<HashMap<[u8; 32], (u64, usize)>> {
    let mut index = HashMap::new();
    let Some((_, current_manifest)) = read_sisr(exe).ok().flatten() else {
        return Ok(index);
    };
    let mut off = footer.payload_offset;
    for chunk in &current_manifest.chunks {
        let len = usize::try_from(chunk.length).map_err(|_| err("chunk length overflow"))?;
        index.insert(chunk.hash, (off, len));
        off = off
            .checked_add(u64::from(chunk.length))
            .ok_or_else(|| err("chunk offset overflow"))?;
    }
    Ok(index)
}

/// The manifest's chunk table must tile the declared payload length exactly.
fn checked_payload_len(manifest: &DeltaManifest) -> io::Result<u64> {
    let sum = manifest.chunks.iter().try_fold(0u64, |acc, c| {
        acc.checked_add(u64::from(c.length))
            .ok_or_else(|| err("chunk lengths overflow"))
    })?;
    if sum != manifest.payload_len {
        return Err(err("manifest payload_len does not match chunk table"));
    }
    Ok(sum)
}

/// Writes one chunk, reusing it from the current binary when its bytes verify,
/// otherwise fetching and hash-verifying it. The mandatory check before any
/// write: `SHA-256(bytes) == chunk.hash`.
fn write_chunk(
    exe: &mut File,
    index: &HashMap<[u8; 32], (u64, usize)>,
    chunk: &crate::manifest::ChunkEntry,
    fetcher: &dyn ChunkFetcher,
    w: &mut File,
    hasher: &mut Sha256,
    stats: &mut SisrUpdateStats,
) -> io::Result<()> {
    let (bytes, reused) = resolve_chunk_bytes(exe, index, chunk, fetcher)?;
    if reused {
        stats.reused_chunks += 1;
        stats.reused_bytes = stats.reused_bytes.saturating_add(bytes.len() as u64);
    } else {
        stats.fetched_chunks += 1;
        stats.fetched_bytes = stats.fetched_bytes.saturating_add(bytes.len() as u64);
    }
    hasher.update(&bytes);
    w.write_all(&bytes)
}

/// The bytes for one chunk, verified against its manifest hash: reused from
/// the current binary when its bytes match, otherwise fetched and checked.
/// Returns whether the chunk was reused. Shared by the write path and the
/// [`target_payload_sha256`] pre-check so both resolve bytes identically.
///
/// [`target_payload_sha256`]: SisrEngine::target_payload_sha256
fn resolve_chunk_bytes(
    exe: &mut File,
    index: &HashMap<[u8; 32], (u64, usize)>,
    chunk: &crate::manifest::ChunkEntry,
    fetcher: &dyn ChunkFetcher,
) -> io::Result<(Vec<u8>, bool)> {
    let expected_len = usize::try_from(chunk.length).map_err(|_| err("chunk length overflow"))?;
    if let Some(&(off, len)) = index.get(&chunk.hash) {
        let candidate = read_at(exe, off, len)?;
        if ct_eq_hash(&sha256_of(&candidate), &chunk.hash) {
            return Ok((candidate, true));
        }
    }
    let bytes = fetch_verified(fetcher, chunk, expected_len)?;
    Ok((bytes, false))
}

fn fetch_verified(
    fetcher: &dyn ChunkFetcher,
    chunk: &crate::manifest::ChunkEntry,
    expected_len: usize,
) -> io::Result<Vec<u8>> {
    let bytes = fetcher.fetch(&chunk.hash, expected_len)?;
    if bytes.len() != expected_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "fetched chunk length mismatch",
        ));
    }
    if !ct_eq_hash(&sha256_of(&bytes), &chunk.hash) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "fetched chunk failed SHA-256 verification",
        ));
    }
    Ok(bytes)
}

/// v3+ footers carry an 8-byte signature-offset prefix (92 bytes total);
/// older versions are 84 bytes.
fn write_footer(w: &mut File, footer: &Footer) -> io::Result<()> {
    if footer.format_version >= 3 {
        w.write_all(&[0u8; 8])?;
    }
    w.write_all(&footer.pack())
}

fn read_at(f: &mut File, off: u64, len: usize) -> io::Result<Vec<u8>> {
    crate::format::read_at(f, off, len)
}

fn err(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap as Map;
    use std::io::Cursor;

    use crate::assembly::assemble_daedalus;
    use crate::sisr_stage::SisrBuildConfig;

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

    /// Stand-in for an unauthenticated update signature (legacy shape).
    const UNSIGNED: [u8; 64] = [0u8; 64];

    /// Fetcher backed by a map of hash → bytes (tests only).
    struct MapFetcher {
        map: Map<[u8; 32], Vec<u8>>,
        fetched: Cell<u64>,
    }

    impl MapFetcher {
        fn new() -> Self {
            Self {
                map: Map::new(),
                fetched: Cell::new(0),
            }
        }

        fn put(&mut self, hash: [u8; 32], bytes: Vec<u8>) {
            self.map.insert(hash, bytes);
        }
    }

    impl ChunkFetcher for MapFetcher {
        fn fetch(&self, hash: &[u8; 32], _length: usize) -> io::Result<Vec<u8>> {
            let bytes =
                self.map.get(hash).cloned().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "chunk not available")
                })?;
            self.fetched
                .set(self.fetched.get().saturating_add(bytes.len() as u64));
            Ok(bytes)
        }

        fn bytes_fetched(&self) -> u64 {
            self.fetched.get()
        }
    }

    fn build_current_bin(dir: &Path, payload: &[u8], meta: &[u8], chunk: usize) -> PathBuf {
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
                meta_bytes: meta,
                encrypt: false,
                squashfs: false,
                target_arch: None,
                sisr: Some(artifacts),
            },
        )
        .unwrap();
        out
    }

    fn read_current_manifest(path: &Path) -> DeltaManifest {
        let data = fs::read(path).unwrap();
        let (_, manifest) = read_sisr(&mut Cursor::new(&data)).unwrap().unwrap();
        manifest
    }

    fn rebuilt_payload(path: &Path) -> Vec<u8> {
        let data = fs::read(path).unwrap();
        let footer = Footer::read_from(&mut Cursor::new(&data)).unwrap();
        crate::format::read_at(
            &mut Cursor::new(&data),
            footer.payload_offset,
            footer.payload_csize as usize,
        )
        .unwrap()
    }

    #[test]
    fn reuse_80_percent_fetches_only_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let payload = random_buf(40_000, 1);
        let meta = br#"{"name":"test"}"#;
        let cur = build_current_bin(tmp.path(), &payload, meta, 4096);

        let cur_manifest = read_current_manifest(&cur);
        assert!(cur_manifest.chunks.len() >= 5, "need multiple chunks");

        let reused_count = cur_manifest.chunks.len() * 4 / 5;
        let mut chunks = cur_manifest.chunks[..reused_count].to_vec();
        let mut fetcher = MapFetcher::new();
        let mut new_total = 0u64;
        for i in reused_count..cur_manifest.chunks.len() {
            let bytes = random_buf(2048 + i * 13, 100 + i as u64);
            let hash = sha256_of(&bytes);
            new_total += bytes.len() as u64;
            fetcher.put(hash, bytes.clone());
            chunks.push(crate::manifest::ChunkEntry {
                hash,
                length: u32::try_from(bytes.len()).unwrap(),
            });
        }
        let target = DeltaManifest {
            version: crate::manifest::VERSION,
            payload_len: chunks.iter().map(|c| u64::from(c.length)).sum(),
            chunks,
        };

        let engine = SisrEngine;
        let (updated, stats) = engine
            .apply_update_with_stats(&cur, &target, &fetcher, &UNSIGNED)
            .unwrap();

        assert_eq!(stats.reused_chunks, reused_count);
        assert_eq!(
            stats.fetched_chunks,
            cur_manifest.chunks.len() - reused_count
        );
        assert_eq!(stats.fetched_bytes, new_total);
        assert_eq!(updated, fs::canonicalize(&cur).unwrap());
        assert_eq!(fetcher.bytes_fetched(), new_total);

        // Verify via the footer hash instead of re-deriving offsets.
        let data = fs::read(&updated).unwrap();
        let footer = Footer::read_from(&mut Cursor::new(&data)).unwrap();
        let rebuilt = rebuilt_payload(&updated);
        let mut h = Sha256::new();
        h.update(&rebuilt);
        h.update(meta);
        let expect: [u8; 32] = h.finalize().into();
        assert_eq!(footer.payload_sha256, expect);
        assert_eq!(rebuilt.len() as u64, target.payload_len);
    }

    #[test]
    fn interrupted_update_leaves_original_binary_intact() {
        let tmp = tempfile::tempdir().unwrap();
        let payload = random_buf(10_000, 2);
        let meta = br#"{"name":"test"}"#;
        let cur = build_current_bin(tmp.path(), &payload, meta, 4096);
        let original = fs::read(&cur).unwrap();

        // Manifest with chunks the fetcher cannot serve -> engine errors after
        // partial writes; the destination must be byte-identical afterwards.
        let bad = DeltaManifest {
            version: crate::manifest::VERSION,
            payload_len: payload.len() as u64,
            chunks: vec![crate::manifest::ChunkEntry {
                hash: sha256_of(b"unavailable"),
                length: 512,
            }],
        };
        let fetcher = MapFetcher::new();
        assert!(SisrEngine
            .apply_update(&cur, &bad, &fetcher, &UNSIGNED)
            .is_err());
        assert_eq!(fs::read(&cur).unwrap(), original, "binary must be intact");
        assert!(
            fs::read_dir(tmp.path()).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp-")),
            "no leftover temp files"
        );
    }

    #[test]
    fn fetched_chunk_with_wrong_hash_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let payload = random_buf(2_000, 3);
        let meta = br#"{"name":"test"}"#;
        let cur = build_current_bin(tmp.path(), &payload, meta, 4096);
        let original = fs::read(&cur).unwrap();

        let want_hash = sha256_of(b"expected-content");
        let mut fetcher = MapFetcher::new();
        fetcher.put(want_hash, b"tampered-content".to_vec());
        let target = DeltaManifest {
            version: crate::manifest::VERSION,
            payload_len: "tampered-content".len() as u64,
            chunks: vec![crate::manifest::ChunkEntry {
                hash: want_hash,
                length: "tampered-content".len() as u32,
            }],
        };

        let res = SisrEngine.apply_update(&cur, &target, &fetcher, &UNSIGNED);
        assert!(res.is_err());
        assert_eq!(fs::read(&cur).unwrap(), original);
    }

    #[test]
    fn manifest_length_mismatch_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let cur = build_current_bin(tmp.path(), b"payload", b"{}", 4096);
        let original = fs::read(&cur).unwrap();
        let bad = DeltaManifest {
            version: crate::manifest::VERSION,
            payload_len: 9999,
            chunks: vec![],
        };
        assert!(SisrEngine
            .apply_update(&cur, &bad, &MapFetcher::new(), &UNSIGNED)
            .is_err());
        assert_eq!(fs::read(&cur).unwrap(), original);
    }

    #[test]
    fn full_fetch_when_current_binary_has_no_sisr_section() {
        let tmp = tempfile::tempdir().unwrap();
        let payload = random_buf(8_000, 5);
        let meta = br#"{"name":"legacy"}"#;
        let out = tmp.path().join("legacy.daedalus");
        crate::assembly::assemble_daedalus(
            &out,
            &crate::assembly::AssemblyInput {
                stub_bytes: b"STUB_DATA_HERE",
                payload: &payload,
                meta_bytes: meta,
                encrypt: false,
                squashfs: false,
                target_arch: None,
                sisr: None,
            },
        )
        .unwrap();

        let hash = sha256_of(&payload);
        let mut fetcher = MapFetcher::new();
        fetcher.put(hash, payload.clone());
        let target = DeltaManifest {
            version: crate::manifest::VERSION,
            payload_len: payload.len() as u64,
            chunks: vec![crate::manifest::ChunkEntry {
                hash,
                length: payload.len() as u32,
            }],
        };
        let (updated, stats) = SisrEngine
            .apply_update_with_stats(&out, &target, &fetcher, &UNSIGNED)
            .unwrap();
        assert_eq!(stats.fetched_chunks, 1);
        assert_eq!(stats.reused_chunks, 0);
        assert_eq!(rebuilt_payload(&updated), payload);
    }

    #[cfg(unix)]
    #[test]
    fn replaced_binary_keeps_the_executable_bit() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let payload = random_buf(8_000, 8);
        let meta = br#"{"name":"test"}"#;
        let cur = build_current_bin(tmp.path(), &payload, meta, 4096);
        assert_ne!(fs::metadata(&cur).unwrap().permissions().mode() & 0o111, 0);

        let mut fetcher = MapFetcher::new();
        let target = DeltaManifest {
            version: crate::manifest::VERSION,
            payload_len: payload.len() as u64,
            chunks: vec![crate::manifest::ChunkEntry {
                hash: sha256_of(&payload),
                length: payload.len() as u32,
            }],
        };
        fetcher.put(sha256_of(&payload), payload.clone());
        let (updated, _) = SisrEngine
            .apply_update_with_stats(&cur, &target, &fetcher, &UNSIGNED)
            .unwrap();
        assert_ne!(
            fs::metadata(&updated).unwrap().permissions().mode() & 0o111,
            0,
            "replaced binary must stay executable"
        );
    }

    /// The pre-swap dry-run (`target_payload_sha256`) predicts the rebuilt
    /// footer's hash exactly — on a mixed delta (reuse + fetch) — without
    /// touching the binary.
    #[test]
    fn dry_run_hash_matches_rebuilt_footer_without_writes() {
        let tmp = tempfile::tempdir().unwrap();
        let payload = random_buf(12_000, 11);
        let meta = br#"{"name":"test"}"#;
        let cur = build_current_bin(tmp.path(), &payload, meta, 4096);
        let cur_manifest = read_current_manifest(&cur);

        // Mixed delta: reuse every chunk but the last, fetch a new tail.
        let mut chunks = cur_manifest.chunks[..cur_manifest.chunks.len() - 1].to_vec();
        let mut fetcher = MapFetcher::new();
        let tail = random_buf(2_048, 77);
        let tail_hash = sha256_of(&tail);
        fetcher.put(tail_hash, tail.clone());
        chunks.push(crate::manifest::ChunkEntry {
            hash: tail_hash,
            length: tail.len() as u32,
        });
        let target = DeltaManifest {
            version: crate::manifest::VERSION,
            payload_len: chunks.iter().map(|c| u64::from(c.length)).sum(),
            chunks,
        };

        let before = fs::read(&cur).unwrap();
        let dry_hash = SisrEngine
            .target_payload_sha256(&cur, &target, &fetcher)
            .unwrap();
        assert_eq!(
            fs::read(&cur).unwrap(),
            before,
            "dry-run must not modify the binary"
        );

        let (updated, _) = SisrEngine
            .apply_update_with_stats(&cur, &target, &fetcher, &UNSIGNED)
            .unwrap();
        let data = fs::read(&updated).unwrap();
        let footer = Footer::read_from(&mut Cursor::new(&data)).unwrap();
        assert_eq!(
            dry_hash, footer.payload_sha256,
            "pre-swap hash must equal the rebuilt footer's hash"
        );
    }

    #[test]
    fn directory_fetcher_reads_hex_hash_files_and_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chunks");
        fs::create_dir_all(&dir).unwrap();
        let hash = sha256_of(b"chunk-bytes");
        fs::write(dir.join(hex::encode(hash)), b"chunk-bytes").unwrap();
        let fetcher = DirectoryChunkFetcher::new(&dir);
        assert_eq!(
            fetcher.fetch(&hash, b"chunk-bytes".len()).unwrap(),
            b"chunk-bytes"
        );
        assert_eq!(fetcher.bytes_fetched(), b"chunk-bytes".len() as u64);
        assert!(fetcher.fetch(&sha256_of(b"missing"), 4).is_err());
    }

    #[test]
    fn corrupted_reused_chunk_falls_back_to_fetch() {
        let tmp = tempfile::tempdir().unwrap();
        let payload = random_buf(16_000, 6);
        let meta = br#"{"name":"test"}"#;
        let cur = build_current_bin(tmp.path(), &payload, meta, 4096);
        let cur_manifest = read_current_manifest(&cur);
        assert!(cur_manifest.chunks.len() >= 2);

        // Corrupt one byte inside the second chunk region of the file.
        let mut data = fs::read(&cur).unwrap();
        let footer = Footer::read_from(&mut Cursor::new(&data)).unwrap();
        let second_off = footer.payload_offset + u64::from(cur_manifest.chunks[0].length) + 3;
        data[second_off as usize] ^= 0xFF;
        fs::write(&cur, &data).unwrap();

        // Fetcher serves the correct bytes for the corrupted chunk.
        let good = &payload[cur_manifest.chunks[0].length as usize
            ..cur_manifest.chunks[0].length as usize + cur_manifest.chunks[1].length as usize];
        let mut fetcher = MapFetcher::new();
        fetcher.put(cur_manifest.chunks[1].hash, good.to_vec());

        let (updated, stats) = SisrEngine
            .apply_update_with_stats(&cur, &cur_manifest, &fetcher, &UNSIGNED)
            .unwrap();
        assert_eq!(stats.reused_chunks, cur_manifest.chunks.len() - 1);
        assert_eq!(stats.fetched_chunks, 1);
        assert_eq!(
            rebuilt_payload(&updated),
            payload,
            "payload must be pristine"
        );
    }

    /// The publisher scenario at engine level: a REAL v1→v2 delta (rewritten
    /// tail → some chunks fetched), signed by the publisher, must produce a
    /// binary that verifies offline against the trusted key — and only it.
    #[test]
    fn applied_update_stays_authentic_at_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let meta = br#"{"name":"test"}"#;

        // v1: what is currently installed. v2: same head, rewritten tail, so
        // content-defined cut points keep the leading chunks reusable.
        let v1 = random_buf(48_000, 21);
        let cur = build_current_bin(tmp.path(), &v1, meta, 4096);
        let mut v2 = v1.clone();
        v2[32_000..].copy_from_slice(&random_buf(16_000, 77));

        // Publisher signs the v2 manifest; every v2 chunk is served and the
        // engine decides what to reuse.
        let key = ed25519_dalek::SigningKey::from_bytes(&[9u8; 32]);
        let config = SisrBuildConfig {
            enabled: true,
            chunk_target_size: 4096,
            signing_key: Some(key.clone()),
        };
        let artifacts = crate::sisr_stage::build_artifacts(&v2, &config).unwrap();
        let mut fetcher = MapFetcher::new();
        let mut pos = 0usize;
        for chunk in &artifacts.manifest.chunks {
            let end = pos + chunk.length as usize;
            fetcher.put(chunk.hash, v2[pos..end].to_vec());
            pos = end;
        }

        let (updated, stats) = SisrEngine
            .apply_update_with_stats(&cur, &artifacts.manifest, &fetcher, &artifacts.signature)
            .unwrap();
        assert!(
            stats.fetched_chunks > 0 && stats.fetched_chunks < artifacts.manifest.chunks.len(),
            "a real delta must mix reuse ({}) and fetch ({})",
            stats.reused_chunks,
            stats.fetched_chunks
        );
        assert_eq!(rebuilt_payload(&updated), v2, "payload must be exactly v2");

        // The rebuilt binary carries the verified signature verbatim...
        let data = fs::read(&updated).unwrap();
        let (ext, manifest) = read_sisr(&mut Cursor::new(&data)).unwrap().unwrap();
        assert_eq!(ext.signature, artifacts.signature);

        // ...verifies offline against the trusted key...
        crate::sisr_stage::verify_embedded_sisr(
            &ext,
            &manifest,
            &rebuilt_payload(&updated),
            &[key.verifying_key()],
        )
        .unwrap();

        // ...and only against a trusted key.
        let rogue = ed25519_dalek::SigningKey::from_bytes(&[8u8; 32]);
        let err = crate::sisr_stage::verify_embedded_sisr(
            &ext,
            &manifest,
            &rebuilt_payload(&updated),
            &[rogue.verifying_key()],
        )
        .unwrap_err();
        assert!(err.to_string().contains("signature verification failed"));
    }
}
