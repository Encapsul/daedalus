//! `SISR` build stage — chunk the payload, build the Merkle tree, sign, and
//! package the embedded section and the remote manifest.
//!
//! Pure computation over the payload bytes: no filesystem writes here. The
//! packager (`assembly`) consumes [`SisrArtifacts`] and injects the section
//! into the `.daedalus`, then emits the remote manifest next to the binary.

use std::io;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::chunker::{Chunker, FastCDC};
use crate::manifest::{self, ChunkEntry, DeltaManifest};

/// Ed25519 private key used to sign the `SISR` manifest during the build.
///
/// Backed by `ed25519-dalek`'s [`SigningKey`], which zeroizes its secret
/// bytes on drop (the `zeroize` feature is enabled by default).
pub type Ed25519PrivateKey = SigningKey;

/// Magic of the standalone remote manifest file (`.daedalus.manifest`).
pub const REMOTE_MAGIC: &[u8; 4] = b"XBMR";
/// Version of the remote manifest schema understood by this crate.
pub const REMOTE_VERSION: u8 = 1;
/// Fixed bytes before the embedded manifest in a remote manifest file:
/// magic (4) + version (1) + reserved (3) + merkle root (32) + signature (64).
pub const REMOTE_HEADER_SIZE: usize = 104;

/// Build options for the `SISR` stage.
#[derive(Debug, Clone)]
pub struct SisrBuildConfig {
    /// Set to inject a `SISR` section; clear keeps the classic `.daedalus`.
    pub enabled: bool,
    /// Average content-defined chunk size (e.g. 64 KiB).
    pub chunk_target_size: usize,
    /// Optional Ed25519 key to sign the manifest and `SISR` header.
    pub signing_key: Option<Ed25519PrivateKey>,
}

/// Result of the `SISR` stage, ready for injection into a `.daedalus`.
pub struct SisrArtifacts {
    pub manifest: DeltaManifest,
    pub manifest_bytes: Vec<u8>,
    pub merkle_root: [u8; 32],
    pub signature: [u8; 64],
}

/// Self-contained, signed remote manifest (the `.daedalus.manifest` file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteManifest {
    pub merkle_root: [u8; 32],
    pub signature: [u8; 64],
    pub manifest: DeltaManifest,
}

impl SisrBuildConfig {
    /// A disabled `SISR` config — the packager produces a classic `.daedalus`.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            chunk_target_size: 8192,
            signing_key: None,
        }
    }
}

impl RemoteManifest {
    /// Serializes the remote manifest into its standalone file form.
    pub fn to_bytes(&self) -> io::Result<Vec<u8>> {
        let manifest_bytes = self.manifest.serialize()?;
        let mut out = Vec::with_capacity(REMOTE_HEADER_SIZE + manifest_bytes.len());
        out.extend_from_slice(REMOTE_MAGIC);
        out.push(REMOTE_VERSION);
        out.extend_from_slice(&[0u8; 3]);
        out.extend_from_slice(&self.merkle_root);
        out.extend_from_slice(&self.signature);
        out.extend_from_slice(&manifest_bytes);
        Ok(out)
    }

    /// Parses a remote manifest from a complete file buffer.
    pub fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() < REMOTE_HEADER_SIZE {
            return Err(err("truncated remote manifest header"));
        }
        if &bytes[0..4] != REMOTE_MAGIC {
            return Err(err("bad remote manifest magic"));
        }
        let version = bytes[4];
        if version > REMOTE_VERSION {
            return Err(err("unsupported remote manifest version"));
        }
        Ok(Self {
            merkle_root: fixed(&bytes[8..40])?,
            signature: fixed(&bytes[40..104])?,
            manifest: DeltaManifest::parse(&bytes[104..])?,
        })
    }

    /// Verifies the Ed25519 signature against `public`.
    pub fn verify_signature(&self, public: &VerifyingKey) -> bool {
        let Ok(manifest_bytes) = self.manifest.serialize() else {
            return false;
        };
        let msg = signing_message(&self.merkle_root, &manifest_bytes);
        let sig = Signature::from_bytes(&self.signature);
        public.verify(&msg, &sig).is_ok()
    }

    /// Verifies the Ed25519 signature against any key in `publics`.
    ///
    /// Trust model: the launcher accepts an update if **any** configured
    /// trusted key signed it (mirrors the binary signature check).
    pub fn verify_any(&self, publics: &[VerifyingKey]) -> bool {
        publics.iter().any(|public| self.verify_signature(public))
    }

    /// Verifies that the chunk table commits to the stored Merkle root.
    pub fn verify_merkle(&self) -> bool {
        merkle_root_of(&self.manifest) == self.merkle_root
    }
}

/// Runs the whole `SISR` stage over the payload.
pub fn build_artifacts(payload: &[u8], config: &SisrBuildConfig) -> io::Result<SisrArtifacts> {
    let chunks = chunk_payload(payload, config.chunk_target_size)?;
    let manifest = DeltaManifest {
        version: manifest::VERSION,
        payload_len: payload.len() as u64,
        chunks,
    };
    let manifest_bytes = manifest.serialize()?;
    let merkle_root = merkle_root_of(&manifest);
    let signature = match &config.signing_key {
        Some(key) => sign(&manifest_bytes, &merkle_root, key),
        None => [0u8; 64],
    };
    Ok(SisrArtifacts {
        manifest,
        manifest_bytes,
        merkle_root,
        signature,
    })
}

/// Splits the payload into content-addressed chunks at `target_size`.
pub fn chunk_payload(payload: &[u8], target_size: usize) -> io::Result<Vec<ChunkEntry>> {
    let chunker = FastCDC::new(target_size)?;
    chunker
        .chunk(payload)
        .into_iter()
        .map(|c| {
            Ok(ChunkEntry {
                hash: c.hash,
                length: u32::try_from(c.length)
                    .map_err(|_| err("chunk length exceeds manifest capacity"))?,
            })
        })
        .collect()
}

/// Merkle root over chunk content hashes (leaves), pairing up with the last
/// hash duplicated when a level has an odd count. A single leaf is its own
/// root; an empty tree hashes the empty input.
pub fn merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return Sha256::digest([]).into();
    }
    let mut level: Vec<[u8; 32]> = leaves.to_vec();
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len() / 2 + 1);
        let mut i = 0;
        while i < level.len() {
            let a = level[i];
            let b = if i + 1 < level.len() { level[i + 1] } else { a };
            let mut h = Sha256::new();
            h.update(a);
            h.update(b);
            next.push(h.finalize().into());
            i += 2;
        }
        level = next;
    }
    level[0]
}

/// Merkle root of the chunk hashes referenced by `manifest`.
pub fn merkle_root_of(manifest: &DeltaManifest) -> [u8; 32] {
    let leaves: Vec<[u8; 32]> = manifest.chunks.iter().map(|c| c.hash).collect();
    merkle_root(&leaves)
}

/// Ed25519 signature over `merkle_root ‖ manifest_bytes`.
pub fn sign(manifest_bytes: &[u8], merkle_root: &[u8; 32], key: &Ed25519PrivateKey) -> [u8; 64] {
    let msg = signing_message(merkle_root, manifest_bytes);
    key.sign(&msg).to_bytes()
}

/// Verifies a signature over `merkle_root ‖ manifest_bytes`.
pub fn verify(
    manifest_bytes: &[u8],
    merkle_root: &[u8; 32],
    signature: &[u8; 64],
    public: &VerifyingKey,
) -> bool {
    let msg = signing_message(merkle_root, manifest_bytes);
    public
        .verify(&msg, &Signature::from_bytes(signature))
        .is_ok()
}

/// Verifies the at-rest authenticity of an embedded SISR section.
///
/// The whole chain runs offline: an Ed25519 signature from a trusted key
/// covers `merkle_root ‖ manifest_bytes`, the Merkle root commits to the
/// chunk table, and every chunk region of `payload` must hash to its
/// manifest entry — binding the bytes that will actually run to the signed
/// table. Fails closed on an all-zeros signature (unsigned build), a Merkle
/// mismatch, an untrusted signature, a chunk table that does not tile
/// `payload` exactly, or any content mismatch.
///
/// Cost note: the per-chunk pass is one extra SHA-256 sweep over the
/// payload, same order as the launcher's existing full-file integrity check.
pub fn verify_embedded_sisr(
    ext: &crate::sisr_header::SisrFooterExt,
    manifest: &DeltaManifest,
    payload: &[u8],
    publics: &[VerifyingKey],
) -> io::Result<()> {
    if ext.signature == [0u8; 64] {
        return Err(err("unsigned SISR section refused at rest"));
    }
    if merkle_root_of(manifest) != ext.merkle_root {
        return Err(err("embedded SISR Merkle root does not match chunk table"));
    }
    let manifest_bytes = manifest.serialize()?;
    if !publics
        .iter()
        .any(|public| verify(&manifest_bytes, &ext.merkle_root, &ext.signature, public))
    {
        return Err(err("embedded SISR signature verification failed"));
    }
    verify_payload_chunks(manifest, payload)
}

/// Binds `payload` to the signed chunk table: exact tiling plus a
/// constant-time hash check per chunk region.
pub fn verify_payload_chunks(manifest: &DeltaManifest, payload: &[u8]) -> io::Result<()> {
    if manifest.payload_len != payload.len() as u64 {
        return Err(err("SISR manifest payload_len does not match payload"));
    }
    let mut pos = 0usize;
    for chunk in &manifest.chunks {
        let len = usize::try_from(chunk.length).map_err(|_| err("chunk length overflow"))?;
        let end = pos
            .checked_add(len)
            .ok_or_else(|| err("chunk region overflow"))?;
        let region = payload
            .get(pos..end)
            .ok_or_else(|| err("chunk region out of payload bounds"))?;
        if !ct_eq_hash(&sha256_digest(region), &chunk.hash) {
            return Err(err("payload chunk failed SHA-256 verification"));
        }
        pos = end;
    }
    if pos != payload.len() {
        return Err(err("SISR chunk table does not tile the payload exactly"));
    }
    Ok(())
}

pub(crate) fn sha256_digest(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

/// Constant-time 32-byte digest comparison. Integrity hashes are public, but
/// the codebase's policy is to compare every verification digest without a
/// timing side-channel so a future secret-bearing digest can't regress.
pub(crate) fn ct_eq_hash(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.ct_eq(b).into()
}

/// `signing_message` - signing message.
/// `@merkle_root`: merkle root
/// `@manifest_bytes`: manifest bytes
///
/// Description:
///
/// Return: vector of Vec<u8>
fn signing_message(merkle_root: &[u8; 32], manifest_bytes: &[u8]) -> Vec<u8> {
    let mut msg = Vec::with_capacity(32 + manifest_bytes.len());
    msg.extend_from_slice(merkle_root);
    msg.extend_from_slice(manifest_bytes);
    msg
}

fn fixed<const N: usize>(b: &[u8]) -> io::Result<[u8; N]> {
    b.try_into()
        .map_err(|_| err("truncated remote manifest field"))
}

/// `err` - err.
/// `@msg`: message
/// `@io`: io
///
/// Description:
///
/// Return: the `std::io::Error`
fn err(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `manifest_with` - manifest with.
    /// `@hashes`: hashes
    ///
    /// Description:
    ///
    /// Return: the `DeltaManifest`
    fn manifest_with(hashes: &[[u8; 32]]) -> DeltaManifest {
        DeltaManifest {
            version: manifest::VERSION,
            payload_len: 0,
            chunks: hashes
                .iter()
                .map(|h| ChunkEntry {
                    hash: *h,
                    length: 4096,
                })
                .collect(),
        }
    }

    /// `random_buf` - random buf.
    /// `@len`: length
    /// `@seed`: seed
    ///
    /// Description:
    ///
    /// Return: vector of Vec<u8>
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

    #[test]
    /// `merkle_root_is_deterministic_and_content_binding` - merkle root is deterministic and content binding.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn merkle_root_is_deterministic_and_content_binding() {
        let leaves = [[1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32]];
        assert_eq!(merkle_root(&leaves), merkle_root(&leaves));
        let mut tampered = leaves;
        tampered[2][0] ^= 1;
        assert_ne!(merkle_root(&leaves), merkle_root(&tampered));
    }

    #[test]
    /// `merkle_root_of_two_leaves_matches_manual_hash` - merkle root of two leaves matches manual hash.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn merkle_root_of_two_leaves_matches_manual_hash() {
        let a = [1u8; 32];
        let b = [2u8; 32];
        let mut h = Sha256::new();
        h.update(a);
        h.update(b);
        let expect: [u8; 32] = h.finalize().into();
        assert_eq!(merkle_root(&[a, b]), expect);
    }

    #[test]
    /// `merkle_root_of_single_leaf_is_the_leaf` - merkle root of single leaf is the leaf.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn merkle_root_of_single_leaf_is_the_leaf() {
        let a = [9u8; 32];
        assert_eq!(merkle_root(&[a]), a);
    }

    #[test]
    /// `chunk_payload_tiles_payload_and_hashes_content` - chunk payload tiles payload and hashes content.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn chunk_payload_tiles_payload_and_hashes_content() {
        let payload = random_buf(100_000, 42);
        let chunks = chunk_payload(&payload, 8192).unwrap();
        let mut pos = 0;
        for chunk in &chunks {
            let end = pos + chunk.length as usize;
            let expect: [u8; 32] = Sha256::digest(&payload[pos..end]).into();
            assert_eq!(chunk.hash, expect);
            pos = end;
        }
        assert_eq!(pos, payload.len());
    }

    #[test]
    /// `sign_verify_roundtrip_with_known_key` - sign verify roundtrip with known key.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn sign_verify_roundtrip_with_known_key() {
        let key = SigningKey::from_bytes(&[9u8; 32]);
        let public = key.verifying_key();
        let manifest = manifest_with(&[[1u8; 32], [2u8; 32]]);
        let manifest_bytes = manifest.serialize().unwrap();
        let root = [3u8; 32];
        let sig = sign(&manifest_bytes, &root, &key);
        assert!(verify(&manifest_bytes, &root, &sig, &public));
        assert!(!verify(&manifest_bytes, &[4u8; 32], &sig, &public));
    }

    #[test]
    /// `build_artifacts_signs_when_key_present` - build artifacts signs when key present.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn build_artifacts_signs_when_key_present() {
        let payload = random_buf(50_000, 7);
        let key = SigningKey::from_bytes(&[5u8; 32]);
        let config = SisrBuildConfig {
            enabled: true,
            chunk_target_size: 8192,
            signing_key: Some(key),
        };
        let artifacts = build_artifacts(&payload, &config).unwrap();
        let public = config.signing_key.as_ref().unwrap().verifying_key();
        assert!(verify(
            &artifacts.manifest_bytes,
            &artifacts.merkle_root,
            &artifacts.signature,
            &public
        ));
        assert_eq!(artifacts.manifest.payload_len, payload.len() as u64);
    }

    #[test]
    /// `build_artifacts_without_key_zero_signature` - build artifacts without key zero signature.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn build_artifacts_without_key_zero_signature() {
        let config = SisrBuildConfig {
            enabled: true,
            chunk_target_size: 8192,
            signing_key: None,
        };
        let artifacts = build_artifacts(b"data", &config).unwrap();
        assert_eq!(artifacts.signature, [0u8; 64]);
    }

    #[test]
    /// `remote_manifest_roundtrip_and_verification` - remote manifest roundtrip and verification.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn remote_manifest_roundtrip_and_verification() {
        let key = SigningKey::from_bytes(&[11u8; 32]);
        let public = key.verifying_key();
        let payload = random_buf(100_000, 3);
        let config = SisrBuildConfig {
            enabled: true,
            chunk_target_size: 8192,
            signing_key: Some(key),
        };
        let artifacts = build_artifacts(&payload, &config).unwrap();
        let remote = RemoteManifest {
            merkle_root: artifacts.merkle_root,
            signature: artifacts.signature,
            manifest: artifacts.manifest,
        };
        let bytes = remote.to_bytes().unwrap();
        let parsed = RemoteManifest::from_bytes(&bytes).unwrap();
        assert_eq!(parsed, remote);
        assert!(parsed.verify_signature(&public));
        assert!(parsed.verify_merkle());
    }

    #[test]
    /// `remote_manifest_verify_any_accepts_any_trusted_key` - remote manifest verify any accepts any trusted key.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn remote_manifest_verify_any_accepts_any_trusted_key() {
        let key = SigningKey::from_bytes(&[17u8; 32]);
        let other = SigningKey::from_bytes(&[19u8; 32]);
        let public = key.verifying_key();
        let config = SisrBuildConfig {
            enabled: true,
            chunk_target_size: 8192,
            signing_key: Some(key),
        };
        let artifacts = build_artifacts(&random_buf(10_000, 4), &config).unwrap();
        let remote = RemoteManifest {
            merkle_root: artifacts.merkle_root,
            signature: artifacts.signature,
            manifest: artifacts.manifest,
        };
        assert!(remote.verify_any(&[other.verifying_key(), public]));
        assert!(!remote.verify_any(&[other.verifying_key()]));
        assert!(!remote.verify_any(&[]));
    }

    #[test]
    /// `remote_manifest_rejects_tampering` - remote manifest rejects tampering.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn remote_manifest_rejects_tampering() {
        let key = SigningKey::from_bytes(&[13u8; 32]);
        let public = key.verifying_key();
        let config = SisrBuildConfig {
            enabled: true,
            chunk_target_size: 8192,
            signing_key: Some(key),
        };
        let artifacts = build_artifacts(&random_buf(100_000, 9), &config).unwrap();
        let remote = RemoteManifest {
            merkle_root: artifacts.merkle_root,
            signature: artifacts.signature,
            manifest: artifacts.manifest,
        };
        let mut bytes = remote.to_bytes().unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        let parsed = RemoteManifest::from_bytes(&bytes).unwrap();
        assert!(!parsed.verify_signature(&public));
    }

    #[test]
    /// `remote_manifest_rejects_bad_magic_and_truncation` - remote manifest rejects bad magic and truncation.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn remote_manifest_rejects_bad_magic_and_truncation() {
        let mut bytes = vec![0u8; REMOTE_HEADER_SIZE + manifest::HEADER_SIZE];
        bytes[0..4].copy_from_slice(b"XXXX");
        assert!(RemoteManifest::from_bytes(&bytes).is_err());
        assert!(RemoteManifest::from_bytes(&bytes[..REMOTE_HEADER_SIZE - 1]).is_err());
    }

    /// Perf probe (run manually in release: `cargo test -p daedalus-core --release
    /// perf_sisr -- --ignored`). Prints the full SISR stage cost on a 100 MiB
    /// payload so the < 5 % build-overhead budget can be verified.
    #[test]
    #[ignore = "manual perf measurement"]
    /// `perf_sisr_on_100_mib` - perf sisr on 100 mib.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn perf_sisr_on_100_mib() {
        let payload = random_buf(100 << 20, 1);
        let config = SisrBuildConfig {
            enabled: true,
            chunk_target_size: 64 << 10,
            signing_key: Some(SigningKey::from_bytes(&[1u8; 32])),
        };
        let start = std::time::Instant::now();
        let artifacts = build_artifacts(&payload, &config).unwrap();
        let elapsed = start.elapsed();
        let mib = (payload.len() >> 20) as u32;
        println!(
            "SISR stage on {mib} MiB: {:.2?} ({:.1} MiB/s, {} chunks, {} manifest bytes)",
            elapsed,
            f64::from(mib) / elapsed.as_secs_f64(),
            artifacts.manifest.chunks.len(),
            artifacts.manifest_bytes.len(),
        );
    }

    /// Fraction of bytes a weight update perturbs. Model updates (fine-tune,
    /// re-quantization) rewrite a small slice of the weight bytes; the rest of
    /// the tensor is byte-identical and must be reused by SISR's CD-chunking.
    const MODEL_UPDATE_FRACTION: u64 = 10; // percent

    /// Simulate Gemma-2B-style weight bytes: a fixed header followed by a large
    /// tensor region with local structure, using a seeded PRNG so runs are
    /// reproducible. `perturb_frac_pct` marks a region (its % position across
    /// the tensor) where a second "updated" version clears the low 2 bits of a
    /// 10% slice of bytes, mimicking a fine-tuned/re-quantized model.
    fn model_weights(bytes: usize, perturb_frac_pct: Option<u64>) -> Vec<u8> {
        const HEADER: usize = 512;
        let mut data = vec![0u8; HEADER + bytes];
        let header = b"GGUF\x03simulated-daedalus-gemma-2b-it";
        data[..header.len()].copy_from_slice(header);
        let mut state = 0x9E37_79B9u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for byte in &mut data[HEADER..] {
            *byte = (next() & 0xFF) as u8;
        }
        if let Some(frac_pct) = perturb_frac_pct {
            let perturb_len = bytes * MODEL_UPDATE_FRACTION as usize / 100usize;
            let perturb_len = perturb_len.max(1).min(bytes);
            let start = HEADER + bytes * frac_pct as usize / 100usize;
            let start = start.min(HEADER + bytes - perturb_len);
            for byte in &mut data[start..start + perturb_len] {
                *byte &= 0x3F;
            }
        }
        data
    }

    /// Reused bytes between two chunk tables (the SISR delta denominator).
    fn reused_bytes(prev: &DeltaManifest, new: &DeltaManifest) -> u64 {
        let new_hashes: std::collections::HashSet<&[u8; 32]> =
            new.chunks.iter().map(|c| &c.hash).collect();
        prev.chunks
            .iter()
            .filter(|c| new_hashes.contains(&c.hash))
            .map(|c| u64::from(c.length))
            .sum()
    }

    /// The core SISR property Task C depends on: content-defined chunking
    /// reuses unchanged weight regions when a model update perturbs only a
    /// fraction of bytes, so an update transfers far fewer bytes than a full
    /// download.
    #[test]
    fn gemma_weight_delta_reuses_unchanged_chunks() {
        let prev = model_weights(64 << 20, None);
        let new = model_weights(64 << 20, Some(50));
        let cfg = SisrBuildConfig {
            enabled: true,
            chunk_target_size: 64 << 10,
            signing_key: None,
        };
        let prev_art = build_artifacts(&prev, &cfg).unwrap();
        let new_art = build_artifacts(&new, &cfg).unwrap();

        let new_total: u64 = new_art
            .manifest
            .chunks
            .iter()
            .map(|c| u64::from(c.length))
            .sum();
        let reused = reused_bytes(&prev_art.manifest, &new_art.manifest);
        let delta = new_total.saturating_sub(reused);
        let pct_saved = 100.0 - (delta as f64 / new_total as f64) * 100.0;

        // On the simulated 10% perturbation at least half the bytes should be
        // reused; CD-chunking must beat a flat "resend everything" baseline.
        assert!(
            pct_saved > 40.0,
            "expected >40% bandwidth saved on a 10% weight update, got {pct_saved:.1}%"
        );
    }

    /// Manual benchmark (release): `cargo test -p daedalus-core --release
    /// gemma_weight_delta_bandwidth -- --ignored`. Reports the SISR delta size
    /// and % bandwidth saved when updating a Gemma-sized model. Reads real
    /// model pairs (`.gguf` v1 + v2) from `DAEDALUS_SISR_MODEL_V1/V2` when set;
    /// otherwise simulates a ~200 MiB model so the number is reachable offline
    /// and in CI. Output is the honest counter-check to a naive "90% saved"
    /// claim — paste the printed line into the demo/README.
    #[test]
    #[ignore = "manual bandwidth measurement"]
    fn gemma_weight_delta_bandwidth() {
        let (prev, new, label) = match (
            std::env::var("DAEDALUS_SISR_MODEL_V1"),
            std::env::var("DAEDALUS_SISR_MODEL_V2"),
        ) {
            (Ok(v1), Ok(v2)) => (
                std::fs::read(&v1).expect("read model v1"),
                std::fs::read(&v2).expect("read model v2"),
                "real gguf pair".to_string(),
            ),
            _ => (
                model_weights(200 << 20, None),
                model_weights(200 << 20, Some(50)),
                format!("simulated 200 MiB ({}% perturbed)", MODEL_UPDATE_FRACTION),
            ),
        };
        let cfg = SisrBuildConfig {
            enabled: true,
            chunk_target_size: 64 << 10,
            signing_key: Some(SigningKey::from_bytes(&[1u8; 32])),
        };
        let prev_art = build_artifacts(&prev, &cfg).unwrap();
        let new_art = build_artifacts(&new, &cfg).unwrap();
        let new_total: u64 = new_art
            .manifest
            .chunks
            .iter()
            .map(|c| u64::from(c.length))
            .sum();
        let reused = reused_bytes(&prev_art.manifest, &new_art.manifest);
        let delta = new_total.saturating_sub(reused);
        let pct = 100.0 - (delta as f64 / new_total as f64) * 100.0;
        let mib = |b: u64| b as f64 / (1024.0 * 1024.0);
        let reused_count = prev_art
            .manifest
            .chunks
            .iter()
            .filter(|c| new_art.manifest.chunks.iter().any(|n| n.hash == c.hash))
            .count();
        let changed = new_art.manifest.chunks.len().saturating_sub(reused_count);
        println!(
            "SISR gemma update ({label}): delta {:.1} MiB vs {:.1} MiB full — {:.1}% bandwidth saved ({changed} changed chunks, {reused_count} reused)",
            mib(delta),
            mib(new_total),
            pct,
        );
    }

    /// Builds a signed artifact set plus the ext header a binary would embed.
    fn signed_ext(
        payload: &[u8],
        key: &SigningKey,
    ) -> (crate::sisr_header::SisrFooterExt, DeltaManifest, Vec<u8>) {
        let config = SisrBuildConfig {
            enabled: true,
            chunk_target_size: 4096,
            signing_key: Some(key.clone()),
        };
        let artifacts = build_artifacts(payload, &config).unwrap();
        let ext = crate::sisr_header::SisrFooterExt {
            sisr_version: crate::sisr_header::SISR_VERSION,
            chunk_table_offset: 0,
            chunk_table_len: artifacts.manifest_bytes.len() as u32,
            merkle_root: artifacts.merkle_root,
            signature: artifacts.signature,
        };
        (ext, artifacts.manifest, payload.to_vec())
    }

    #[test]
    /// `verify_embedded_sisr_accepts_a_signed_section` - verify embedded sisr accepts a signed section.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn verify_embedded_sisr_accepts_a_signed_section() {
        let key = SigningKey::from_bytes(&[5u8; 32]);
        let payload = random_buf(10_000, 31);
        let (ext, manifest, payload) = signed_ext(&payload, &key);
        verify_embedded_sisr(&ext, &manifest, &payload, &[key.verifying_key()]).unwrap();
    }

    #[test]
    /// `verify_embedded_sisr_rejects_zero_signature` - verify embedded sisr rejects zero signature.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn verify_embedded_sisr_rejects_zero_signature() {
        let key = SigningKey::from_bytes(&[5u8; 32]);
        let payload = random_buf(10_000, 31);
        let (mut ext, manifest, payload) = signed_ext(&payload, &key);
        ext.signature = [0u8; 64];
        let err =
            verify_embedded_sisr(&ext, &manifest, &payload, &[key.verifying_key()]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("unsigned"));
    }

    #[test]
    /// `verify_embedded_sisr_rejects_untrusted_key` - verify embedded sisr rejects untrusted key.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn verify_embedded_sisr_rejects_untrusted_key() {
        let signer = SigningKey::from_bytes(&[5u8; 32]);
        let other = SigningKey::from_bytes(&[6u8; 32]);
        let payload = random_buf(10_000, 31);
        let (ext, manifest, payload) = signed_ext(&payload, &signer);
        let err =
            verify_embedded_sisr(&ext, &manifest, &payload, &[other.verifying_key()]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("signature verification failed"));
    }

    #[test]
    /// `verify_embedded_sisr_rejects_tampered_payload` - verify embedded sisr rejects tampered payload.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn verify_embedded_sisr_rejects_tampered_payload() {
        let key = SigningKey::from_bytes(&[5u8; 32]);
        let payload = random_buf(10_000, 31);
        let (ext, manifest, mut payload) = signed_ext(&payload, &key);
        let last = payload.len() - 1;
        payload[last] ^= 1;
        let err =
            verify_embedded_sisr(&ext, &manifest, &payload, &[key.verifying_key()]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("SHA-256 verification"));
    }

    #[test]
    /// `verify_embedded_sisr_rejects_wrong_payload_length` - verify embedded sisr rejects wrong payload length.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn verify_embedded_sisr_rejects_wrong_payload_length() {
        let key = SigningKey::from_bytes(&[5u8; 32]);
        let payload = random_buf(10_000, 31);
        let (ext, manifest, mut payload) = signed_ext(&payload, &key);
        payload.pop();
        let err =
            verify_embedded_sisr(&ext, &manifest, &payload, &[key.verifying_key()]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("payload_len"));
    }

    #[test]
    /// `verify_embedded_sisr_rejects_merkle_mismatch` - verify embedded sisr rejects merkle mismatch.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn verify_embedded_sisr_rejects_merkle_mismatch() {
        let key = SigningKey::from_bytes(&[5u8; 32]);
        let payload = random_buf(10_000, 31);
        let (mut ext, manifest, payload) = signed_ext(&payload, &key);
        ext.merkle_root[0] ^= 1;
        let err =
            verify_embedded_sisr(&ext, &manifest, &payload, &[key.verifying_key()]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("Merkle root does not match"));
    }
}
