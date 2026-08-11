//! Cryptographic verification for the xbin launcher stub.
//!
//! Provides Ed25519 signature verification, SHA-256 integrity checks,
//! AES-256-GCM decryption, HKDF key derivation, and payload layer slicing.

use std::io;
use std::path::PathBuf;

use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
use ed25519_dalek::{Signature, Verifier};
use sha2::{Digest, Sha256};

use crate::{CryptoMeta, Footer, Layer};
use xbin_core::encrypt::{decrypt_chunks, hkdf_derive_key as core_hkdf_derive_key, NONCE_LEN};
use xbin_core::format::{SIG_BLOCK_SIZE, SIG_LEN};

/// Verify Ed25519 signature: `Ed25519_verify(SHA256(payload‖meta‖footer), sig, public_key)`.
///
/// Trusted public keys are read from `~/.xbin/trusted-keys/` (or `$XBIN_TRUSTED_DIR`).
/// The launcher accepts the file if **any** trusted key verifies the signature.
pub fn verify_ed25519<R: std::io::Read + std::io::Seek>(
    footer: &Footer,
    exe: &mut R,
    payload: &[u8],
    meta_bytes: &[u8],
) -> io::Result<()> {
    // Read signature block: [sig_size: u32le][signature: 64 bytes]
    let sig_data = crate::read_at(exe, footer.sig_offset, SIG_BLOCK_SIZE)?;
    let size_bytes: [u8; 4] = sig_data[0..4]
        .try_into()
        .map_err(|_| crate::err("signature block too small"))?;
    let sig_size = u32::from_le_bytes(size_bytes) as usize;
    if sig_size != SIG_LEN {
        return Err(crate::err("invalid Ed25519 signature size"));
    }
    let sig_bytes: &[u8; 64] = sig_data[4..SIG_BLOCK_SIZE]
        .try_into()
        .map_err(|_| crate::err("invalid signature block size"))?;

    // The digest covers the footer (pack_full) as well as payload‖meta: the
    // footer's format_version and FLAG_SIGNED decide whether the signature is
    // consulted at all, so omitting it would let an attacker downgrade the
    // file to v2 and have the signature silently skipped.
    let mut hasher = Sha256::new();
    hasher.update(payload);
    hasher.update(meta_bytes);
    hasher.update(&footer.pack_full());
    let hash = hasher.finalize();

    // Parse signature once.
    let sig = Signature::from_bytes(sig_bytes);

    let keys = load_trusted_keys()?;
    if !keys.iter().any(|k| k.verify(&hash, &sig).is_ok()) {
        return Err(crate::err("Ed25519 signature verification failed"));
    }
    Ok(())
}

/// Loads every 32-byte Ed25519 public key from the trusted keys directory.
/// Malformed entries are skipped so a stray file can never disable verification.
pub fn load_trusted_keys() -> io::Result<Vec<ed25519_dalek::VerifyingKey>> {
    let trusted_dir = trusted_keys_dir();
    if !trusted_dir.exists() {
        return Err(crate::err(
            "trusted keys directory not found; cannot verify signature",
        ));
    }
    let rd = std::fs::read_dir(&trusted_dir)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("reading trusted keys: {e}")))?;
    let mut keys = Vec::new();
    for entry in rd.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let Ok(raw) = std::fs::read(entry.path()) else {
            continue;
        };
        if raw.len() != 32 {
            continue;
        }
        let Ok(key_arr) = <[u8; 32]>::try_from(raw) else {
            continue;
        };
        if let Ok(key) = ed25519_dalek::VerifyingKey::from_bytes(&key_arr) {
            keys.push(key);
        }
    }
    Ok(keys)
}

/// Return the directory where trusted Ed25519 public keys are stored.
/// Override via `$XBIN_TRUSTED_DIR`; default `~/.xbin/trusted-keys/`.
pub fn trusted_keys_dir() -> PathBuf {
    xbin_core::paths::trusted_keys_dir()
}

// ---------------------------------------------------------------------------
// Cache key (v2)
// ---------------------------------------------------------------------------

/// Compute a SHA-256 cache key from layer digests.
pub fn cache_key_v2(layers: &[Layer]) -> String {
    let mut h = Sha256::new();
    for l in layers {
        h.update(l.sha256.as_bytes());
    }
    hex::encode(h.finalize())
}

/// Slice the payload into per-layer blobs. Returns a single-element vec when
/// the binary is not layered (v2 plain).
pub fn slice_layers<'a>(
    payload: &'a [u8],
    region_offset: u64,
    meta: &crate::Metadata,
    layered: bool,
) -> io::Result<Vec<&'a [u8]>> {
    if !layered {
        return Ok(vec![payload]);
    }
    meta.layers
        .iter()
        .map(|l| {
            let start = (l.offset - region_offset) as usize;
            let end = start
                .checked_add(l.csize as usize)
                .ok_or_else(|| crate::err("layer size overflow"))?;
            payload
                .get(start..end)
                .ok_or_else(|| crate::err("layer extends beyond payload boundary"))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// SHA-256 integrity
// ---------------------------------------------------------------------------

/// Verify a SHA-256 digest in constant time.
///
/// Uses an XOR-fold accumulation instead of early-exit comparison to prevent
/// timing side-channels on the integrity digest.
#[allow(dead_code)]
pub fn verify_sha256(data: &[u8], expected: &[u8; 32]) -> io::Result<()> {
    let mut h = Sha256::new();
    h.update(data);
    ct_eq_sha256(&h.finalize(), expected)
}

/// Verify SHA-256 over two non-contiguous slices without cloning.
///
/// Same integrity check as `verify_sha256` but streams `part1` and `part2`
/// into the hasher separately, avoiding a heap allocation that would
/// duplicate the payload in memory.
pub fn verify_sha256_parts(part1: &[u8], part2: &[u8], expected: &[u8; 32]) -> io::Result<()> {
    let mut h = Sha256::new();
    h.update(part1);
    h.update(part2);
    let got = h.finalize();
    ct_eq_sha256(&got, expected)
}

/// Constant-time comparison of a digest against the expected value.
fn ct_eq_sha256(got: &sha2::digest::Output<sha2::Sha256>, expected: &[u8; 32]) -> io::Result<()> {
    let mut acc = 0u8;
    for (a, b) in got.iter().zip(expected) {
        acc |= a ^ b;
    }
    if acc != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "payload integrity check failed (SHA-256 mismatch)",
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// AES-256-GCM decryption
// ---------------------------------------------------------------------------

/// Derive a 32-byte AES key from the encryption key via HKDF-SHA256.
pub fn hkdf_derive_key(encryption_key: &[u8], salt: &[u8; 32]) -> io::Result<[u8; 32]> {
    let key: &[u8; 32] = encryption_key
        .try_into()
        .map_err(|_| crate::err("encryption key must be exactly 32 bytes"))?;
    core_hkdf_derive_key(key, salt)
        .map_err(|e| crate::err(&format!("HKDF key derivation failed: {e}")))
        .map(|key| *key)
}

/// Decrypt an AES-256-GCM payload.
///
/// The encryption key is stored in metadata. We derive the AES key from it
/// via HKDF, then decrypt. The encryption key is separate from the Ed25519
/// signing seed — it does not protect authenticity (that is the signing
/// key's job), so callers must verify the signature before trusting the
/// decrypted payload.
///
/// Ciphertext layout: [plaintext bytes][16-byte GCM tag]
pub fn decrypt_aes_gcm(ciphertext: &[u8], crypto: &CryptoMeta) -> io::Result<Vec<u8>> {
    let encryption_key = hex_decode(&crypto.encryption_key_hex)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad encryption key hex"))?;
    if encryption_key.len() != 32 {
        return Err(crate::err("encryption key must be 32 bytes"));
    }

    // Decode the encryption salt from metadata
    let salt_bytes = hex_decode(&crypto.encryption_salt_hex)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad encryption salt hex"))?;
    let salt: [u8; 32] = salt_bytes
        .try_into()
        .map_err(|_| crate::err("encryption salt must be exactly 32 bytes"))?;

    let aes_key = hkdf_derive_key(&encryption_key, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&aes_key)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("AES init: {e}")))?;

    let nonce_bytes = hex_decode(&crypto.nonce_hex)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad nonce hex"))?;
    let nonce = Nonce::from_slice(&nonce_bytes);

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("AES decrypt: {e}")))
}

/// Decrypt an AES-256-GCM payload that was encrypted in per-chunk mode
/// (`encrypt_chunks` in `xbin-core/src/encrypt.rs`).
///
/// Each chunk was encrypted with an independent key derived via HKDF from the
/// encryption key, salt, and chunk index. The ciphertext layout is
/// `[ct_chunk_0 || tag_0][ct_chunk_1 || tag_1]...`. `chunk_sizes` gives the
/// plaintext length of each chunk in order.
pub fn chunked_decrypt_aes_gcm(
    ciphertext: &[u8],
    crypto: &CryptoMeta,
    chunk_sizes: &[usize],
) -> io::Result<Vec<u8>> {
    let encryption_key = hex_decode(&crypto.encryption_key_hex)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad encryption key hex"))?;
    let encryption_key: [u8; 32] = encryption_key
        .try_into()
        .map_err(|_| crate::err("encryption key must be 32 bytes"))?;

    let salt_bytes = hex_decode(&crypto.encryption_salt_hex)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad encryption salt hex"))?;
    let salt: [u8; 32] = salt_bytes
        .try_into()
        .map_err(|_| crate::err("encryption salt must be exactly 32 bytes"))?;

    let nonce_bytes = hex_decode(&crypto.nonce_hex)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad nonce hex"))?;
    let mut base_nonce = [0u8; NONCE_LEN];
    base_nonce.copy_from_slice(&nonce_bytes);

    decrypt_chunks(ciphertext, &encryption_key, &salt, &base_nonce, chunk_sizes)
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_sha256_accepts_matching_digest() {
        let data = b"hello";
        let hash = Sha256::digest(data);
        let expected: [u8; 32] = hash.into();
        assert!(verify_sha256(data, &expected).is_ok());
    }

    #[test]
    fn verify_sha256_rejects_mismatch() {
        let data = b"hello";
        let mut expected = [0u8; 32];
        expected[0] = 1;
        assert!(verify_sha256(data, &expected).is_err());
    }

    #[test]
    fn hex_decode_roundtrip() {
        let hex_str = "deadbeef";
        let decoded = hex_decode(hex_str).unwrap();
        assert_eq!(decoded, vec![0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn hex_decode_rejects_odd_length() {
        assert!(hex_decode("abc").is_none());
    }
}
