//! `SISR` build stage — chunk the payload, build the Merkle tree, sign, and
//! package the embedded section and the remote manifest.
//!
//! Pure computation over the payload bytes: no filesystem writes here. The
//! packager (`assembly`) consumes [`SisrArtifacts`] and injects the section
//! into the `.erebus`, then emits the remote manifest next to the binary.

use std::io;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::chunker::{Chunker, FastCDC};
use crate::manifest::{self, ChunkEntry, DeltaManifest};

/// Ed25519 private key used to sign the `SISR` manifest during the build.
///
/// Backed by `ed25519-dalek`'s [`SigningKey`], which zeroizes its secret
/// bytes on drop (the `zeroize` feature is enabled by default).
pub type Ed25519PrivateKey = SigningKey;

/// Magic of the standalone remote manifest file (`.erebus.manifest`).
pub const REMOTE_MAGIC: &[u8; 4] = b"XBMR";
/// Version of the remote manifest schema understood by this crate.
pub const REMOTE_VERSION: u8 = 1;
/// Fixed bytes before the embedded manifest in a remote manifest file:
/// magic (4) + version (1) + reserved (3) + merkle root (32) + signature (64).
pub const REMOTE_HEADER_SIZE: usize = 104;

/// Build options for the `SISR` stage.
#[derive(Debug, Clone)]
pub struct SisrBuildConfig {
    /// Set to inject a `SISR` section; clear keeps the classic `.erebus`.
    pub enabled: bool,
    /// Average content-defined chunk size (e.g. 64 KiB).
    pub chunk_target_size: usize,
    /// Optional Ed25519 key to sign the manifest and `SISR` header.
    pub signing_key: Option<Ed25519PrivateKey>,
}

/// Result of the `SISR` stage, ready for injection into a `.erebus`.
pub struct SisrArtifacts {
    pub manifest: DeltaManifest,
    pub manifest_bytes: Vec<u8>,
    pub merkle_root: [u8; 32],
    pub signature: [u8; 64],
}

/// Self-contained, signed remote manifest (the `.erebus.manifest` file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteManifest {
    pub merkle_root: [u8; 32],
    pub signature: [u8; 64],
    pub manifest: DeltaManifest,
}

impl SisrBuildConfig {
    /// A disabled `SISR` config — the packager produces a classic `.erebus`.
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

fn err(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn merkle_root_is_deterministic_and_content_binding() {
        let leaves = [[1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32]];
        assert_eq!(merkle_root(&leaves), merkle_root(&leaves));
        let mut tampered = leaves;
        tampered[2][0] ^= 1;
        assert_ne!(merkle_root(&leaves), merkle_root(&tampered));
    }

    #[test]
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
    fn merkle_root_of_single_leaf_is_the_leaf() {
        let a = [9u8; 32];
        assert_eq!(merkle_root(&[a]), a);
    }

    #[test]
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
    fn remote_manifest_rejects_bad_magic_and_truncation() {
        let mut bytes = vec![0u8; REMOTE_HEADER_SIZE + manifest::HEADER_SIZE];
        bytes[0..4].copy_from_slice(b"XXXX");
        assert!(RemoteManifest::from_bytes(&bytes).is_err());
        assert!(RemoteManifest::from_bytes(&bytes[..REMOTE_HEADER_SIZE - 1]).is_err());
    }

    /// Perf probe (run manually in release: `cargo test -p erebus-core --release
    /// perf_sisr -- --ignored`). Prints the full SISR stage cost on a 100 MiB
    /// payload so the < 5 % build-overhead budget can be verified.
    #[test]
    #[ignore = "manual perf measurement"]
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
}
