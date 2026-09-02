//! Cryptographic verification for the daedalus launcher stub.
//!
//! Provides Ed25519 signature verification and SHA-256 integrity checks.

use std::io;
use std::path::PathBuf;

use ed25519_dalek::{Signature, Verifier};
use sha2::{Digest, Sha256};

use crate::Footer;
use daedalus_core::format::{SIG_BLOCK_SIZE, SIG_LEN};

/// Verify Ed25519 signature: `Ed25519_verify(SHA256(payload‖meta‖footer), sig, public_key)`.
///
/// Trusted public keys are read from `~/.daedalus/trusted-keys/` (or `$DAEDALUS_TRUSTED_DIR`).
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
    hasher.update(footer.pack_full());
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
/// Returns an empty vector when the directory does not exist, rather than erroring,
/// so that signed files gracefully fail verification instead of refusing to launch.
pub fn load_trusted_keys() -> io::Result<Vec<ed25519_dalek::VerifyingKey>> {
    let trusted_dir = trusted_keys_dir();
    if !trusted_dir.exists() {
        return Ok(Vec::new());
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
/// Override via `$DAEDALUS_TRUSTED_DIR`; default `~/.daedalus/trusted-keys/`.
pub fn trusted_keys_dir() -> PathBuf {
    daedalus_core::paths::trusted_keys_dir()
}

// ---------------------------------------------------------------------------
// SHA-256 integrity
// ---------------------------------------------------------------------------

/// Verify a SHA-256 digest in constant time.
///
/// Uses an XOR-fold accumulation instead of early-exit comparison to prevent
/// timing side-channels on the integrity digest.
#[allow(dead_code)]
/// `verify_sha256` - verify a SHA-256 digest in constant time.
/// @data: data
/// @expected: expected
///
/// Description:
/// Hashes data with SHA-256 and compares against expected using constant-time XOR fold.
///
/// Return: Result containing `io::Result<()>`
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// `verify_sha256_accepts_matching_digest` - verify sha256 accepts matching digest.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn verify_sha256_accepts_matching_digest() {
        let data = b"hello";
        let hash = Sha256::digest(data);
        let expected: [u8; 32] = hash.into();
        assert!(verify_sha256(data, &expected).is_ok());
    }

    #[test]
    /// `verify_sha256_rejects_mismatch` - verify sha256 rejects mismatch.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn verify_sha256_rejects_mismatch() {
        let data = b"hello";
        let mut expected = [0u8; 32];
        expected[0] = 1;
        assert!(verify_sha256(data, &expected).is_err());
    }
}
