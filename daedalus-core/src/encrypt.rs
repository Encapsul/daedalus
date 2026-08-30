//! Payload encryption: AES-256-GCM with HKDF-SHA256 key derivation
//! from a random encryption key. Supports full-payload and per-chunk modes.
use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use hkdf::Hkdf;
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::Sha256;
use std::io;
use zeroize::{Zeroize, Zeroizing};

pub const NONCE_LEN: usize = 12;
const HKDF_INFO: &[u8] = b"aes-256-gcm-key";
pub const CHUNK_TAG_LEN: usize = 16;

#[derive(Debug, Clone)]
pub struct EncryptMetadata {
    pub salt: [u8; 32], // Random salt for HKDF
    pub nonce: [u8; NONCE_LEN],
    pub tag_offset: usize,
}

/// `hkdf_derive_key` - hkdf derive key.
///
/// Description:
///
/// Return: nothing
pub fn hkdf_derive_key(
    encryption_key: &[u8; 32],
    salt: &[u8; 32],
) -> anyhow::Result<Zeroizing<[u8; 32]>> {
    let hk = Hkdf::<Sha256>::new(Some(salt), encryption_key);
    let mut key = Zeroizing::new([0u8; 32]);
    hk.expand(HKDF_INFO, &mut key[..])
        .map_err(|e| anyhow::anyhow!("HKDF expand failed: {e}"))?;
    Ok(key)
}

/// `encrypt_payload` - encrypt payload.
///
/// Description:
///
/// Return: nothing
pub fn encrypt_payload(
    plaintext: &[u8],
    encryption_key: &[u8; 32],
) -> anyhow::Result<(Vec<u8>, EncryptMetadata)> {
    // Generate a random salt per encryption operation to ensure unique key derivation
    let mut salt = [0u8; 32];
    OsRng.fill_bytes(&mut salt);

    let key = hkdf_derive_key(encryption_key, &salt)?;
    let cipher =
        Aes256Gcm::new_from_slice(&key[..]).map_err(|e| anyhow::anyhow!("aes key init: {e}"))?;

    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from(nonce_bytes);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| anyhow::anyhow!("aes encrypt: {e}"))?;
    let metadata = EncryptMetadata {
        salt,
        nonce: nonce_bytes,
        tag_offset: plaintext.len(),
    };
    // `Nonce::from`, metadata assignment, and `encrypt` all copied the nonce bytes;
    // scrub the local copy that is no longer needed.
    nonce_bytes.zeroize();

    Ok((ciphertext, metadata))
}

/// `decrypt_payload` - decrypt payload.
///
/// Description:
///
/// Return: nothing
pub fn decrypt_payload(
    ciphertext: &[u8],
    encryption_key: &[u8; 32],
    nonce: &[u8; NONCE_LEN],
    salt: &[u8; 32],
) -> anyhow::Result<Vec<u8>> {
    let key = hkdf_derive_key(encryption_key, salt)?;
    let cipher =
        Aes256Gcm::new_from_slice(&key[..]).map_err(|e| anyhow::anyhow!("aes key init: {e}"))?;
    let nonce = Nonce::from(*nonce);

    let plaintext = cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("aes decrypt: {e}"))?;
    Ok(plaintext)
}

/// Generate a random 32-byte encryption salt.
pub fn generate_salt() -> [u8; 32] {
    let mut salt = [0u8; 32];
    OsRng.fill_bytes(&mut salt);
    salt
}

/// Generate a random 32-byte encryption key for AES-256-GCM.
///
/// This key is independent of the Ed25519 signing seed and is used only
/// for HKDF key derivation + AES encryption.  It is safe to embed in the
/// binary metadata; compromising it does not reveal the signing seed.
pub fn generate_encryption_key() -> Zeroizing<[u8; 32]> {
    generate_salt().into()
}

/// Generate a random 12-byte AES-GCM nonce.
pub fn generate_nonce() -> [u8; NONCE_LEN] {
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    nonce
}

/// Derive a per-chunk AES-256-GCM key from the encryption key, salt, and chunk index.
///
/// Uses HKDF-SHA256 with `HKDF_INFO` and chunk index encoded as big-endian u64
/// as the info field, ensuring each chunk gets an independent key.
fn derive_chunk_key(
    encryption_key: &[u8; 32],
    salt: &[u8; 32],
    chunk_index: u64,
) -> io::Result<Zeroizing<[u8; 32]>> {
    let mut info = Vec::with_capacity(HKDF_INFO.len() + 8);
    info.extend_from_slice(HKDF_INFO);
    info.extend_from_slice(&chunk_index.to_be_bytes());
    let hkdf = Hkdf::<Sha256>::new(Some(salt), encryption_key);
    let mut key = Zeroizing::new([0u8; 32]);
    hkdf.expand(&info, &mut key[..])
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("HKDF expand failed: {e}")))?;
    Ok(key)
}

/// Build a per-chunk GCM nonce from `base_nonce` and `chunk_index`.
///
/// Layout: `[base_nonce[0..4]; chunk_index: u64 be]`.
/// Each chunk gets a unique nonce by encoding the big-endian chunk index
/// in the last 8 bytes, so nonce uniqueness is guaranteed without relying
/// solely on HKDF key independence.
fn chunk_nonce(base_nonce: &[u8; NONCE_LEN], chunk_index: u64) -> [u8; NONCE_LEN] {
    let mut nonce = [0u8; NONCE_LEN];
    nonce[0..4].copy_from_slice(&base_nonce[0..4]);
    nonce[4..12].copy_from_slice(&chunk_index.to_be_bytes());
    nonce
}

/// Encrypt plaintext in content-defined chunks using per-chunk AES-256-GCM keys.
///
/// Each chunk gets an independent key derived via HKDF-SHA256(encryption_key, salt,
/// chunk `chunk_index`). The ciphertext for each chunk is the plaintext encrypted with
/// its key, with the 16-byte GCM tag appended. The returned `Vec` is the
/// concatenation of all ciphertext+tag blocks, ready to be sliced by SISR
/// chunk boundaries.
///
/// `chunk_sizes` specifies the plaintext length of each chunk in order; the
/// sum must equal `plaintext.len()`.
pub fn encrypt_chunks(
    plaintext: &[u8],
    encryption_key: &[u8; 32],
    salt: &[u8; 32],
    base_nonce: &[u8; NONCE_LEN],
    chunk_sizes: &[usize],
) -> io::Result<Vec<u8>> {
    let tag_bytes = chunk_sizes.len().saturating_mul(CHUNK_TAG_LEN);
    let capacity = plaintext.len().saturating_add(tag_bytes);
    let mut ciphertext = Vec::with_capacity(capacity);
    let mut offset = 0usize;
    for (i, &size) in chunk_sizes.iter().enumerate() {
        let end = offset.checked_add(size).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("chunk {i} offset overflow"),
            )
        })?;
        if end > plaintext.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("chunk {i} size {size} exceeds remaining plaintext"),
            ));
        }
        let chunk_plaintext = &plaintext[offset..end];
        let key = derive_chunk_key(encryption_key, salt, i as u64)?;
        let cipher = Aes256Gcm::new_from_slice(&key[..])
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("aes init: {e}")))?;
        let nonce = Nonce::from(chunk_nonce(base_nonce, i as u64));
        let ct = cipher
            .encrypt(&nonce, chunk_plaintext)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("aes encrypt: {e}")))?;
        ciphertext.extend_from_slice(&ct);
        offset += size;
    }
    if offset != plaintext.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("chunk sizes sum to {offset}, expected {}", plaintext.len()),
        ));
    }
    Ok(ciphertext)
}

/// Decrypt AES-256-GCM ciphertext that was produced by `encrypt_chunks`.
///
/// `ciphertext` is the concatenation of `(encrypted_chunk || 16-byte GCM tag)`
/// for each chunk. `chunk_sizes` gives the plaintext length of each chunk in
/// order; the function derives the per-chunk key, decrypts, verifies the tag,
/// and returns the reassembled plaintext.
///
/// Returns an error if any chunk fails authentication (wrong key, corrupted
/// ciphertext, or wrong chunk index).
pub fn decrypt_chunks(
    ciphertext: &[u8],
    encryption_key: &[u8; 32],
    salt: &[u8; 32],
    base_nonce: &[u8; NONCE_LEN],
    chunk_sizes: &[usize],
) -> io::Result<Vec<u8>> {
    let total_pt: usize = chunk_sizes.iter().copied().sum();
    let mut plaintext = Vec::with_capacity(total_pt);
    let mut offset = 0usize;
    for (i, &plaintext_len) in chunk_sizes.iter().enumerate() {
        let ct_len = plaintext_len.saturating_add(CHUNK_TAG_LEN);
        let end = offset.checked_add(ct_len).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("chunk {i}: offset overflow"),
            )
        })?;
        if end > ciphertext.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("chunk {i}: ciphertext truncated (need {ct_len} bytes at offset {offset})"),
            ));
        }
        let chunk_ct = &ciphertext[offset..end];
        let key = derive_chunk_key(encryption_key, salt, i as u64)?;
        let cipher = Aes256Gcm::new_from_slice(&key[..])
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("aes init: {e}")))?;
        let nonce = Nonce::from(chunk_nonce(base_nonce, i as u64));
        let pt = cipher.decrypt(&nonce, chunk_ct).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("aes decrypt chunk {i}: {e}"),
            )
        })?;
        if pt.len() != plaintext_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "chunk {i}: decrypted length {} != expected {plaintext_len}",
                    pt.len()
                ),
            ));
        }
        plaintext.extend_from_slice(&pt);
        offset += ct_len;
    }
    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// `test_hkdf_deterministic` - test hkdf deterministic.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn test_hkdf_deterministic() {
        let seed = [0x42u8; 32];
        let salt = [0x01u8; 32]; // Deterministic salt for test
        let key1 = hkdf_derive_key(&seed, &salt).unwrap();
        let key2 = hkdf_derive_key(&seed, &salt).unwrap();
        assert_eq!(key1, key2);
        assert_eq!(key1.len(), 32);
    }

    #[test]
    /// `test_hkdf_wrong_length_panics` - test hkdf wrong length panics.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn test_hkdf_wrong_length_panics() {
        let seed = vec![0u8; 16];
        let result: Result<&[u8; 32], _> = seed.as_slice().try_into();
        assert!(result.is_err());
    }

    #[test]
    /// `test_roundtrip` - test roundtrip.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn test_roundtrip() {
        let seed = [0xABu8; 32];
        let plaintext = b"hello daedalus encryption test data";
        let (ciphertext, meta) = encrypt_payload(plaintext, &seed).unwrap();

        assert_eq!(ciphertext.len(), plaintext.len() + 16);
        assert_eq!(meta.nonce.len(), NONCE_LEN);
        assert_eq!(meta.tag_offset, plaintext.len());

        let decrypted = decrypt_payload(&ciphertext, &seed, &meta.nonce, &meta.salt).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    /// `test_different_seeds_produce_different_ciphertext` - test different seeds produce different ciphertext.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn test_different_seeds_produce_different_ciphertext() {
        let seed1 = [0x01u8; 32];
        let seed2 = [0x02u8; 32];
        let plaintext = b"same plaintext both times";

        let (ct1, _) = encrypt_payload(plaintext, &seed1).unwrap();
        let (ct2, _) = encrypt_payload(plaintext, &seed2).unwrap();

        assert_ne!(ct1, ct2);
    }

    #[test]
    /// `test_decrypt_wrong_key_fails` - test decrypt wrong key fails.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn test_decrypt_wrong_key_fails() {
        let seed1 = [0x01u8; 32];
        let seed2 = [0x02u8; 32];
        let plaintext = b"secret data";

        let (ciphertext, meta) = encrypt_payload(plaintext, &seed1).unwrap();
        let result = decrypt_payload(&ciphertext, &seed2, &meta.nonce, &meta.salt);

        assert!(result.is_err());
    }
    #[test]
    /// `test_chunked_roundtrip` - test chunked roundtrip.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn test_chunked_roundtrip() {
        let seed = [0xABu8; 32];
        let salt = [0xCDu8; 32];
        let mut nonce = [0u8; NONCE_LEN];
        nonce[4..12].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let plaintext =
            b"hello daedalus chunked encryption test data, this is longer than one chunk";
        let chunk_sizes = vec![8, 16, plaintext.len() - 24];
        let ciphertext = encrypt_chunks(plaintext, &seed, &salt, &nonce, &chunk_sizes).unwrap();
        let decrypted = decrypt_chunks(&ciphertext, &seed, &salt, &nonce, &chunk_sizes).unwrap();
        assert_eq!(decrypted, plaintext);
        assert_eq!(
            ciphertext.len(),
            plaintext.len() + chunk_sizes.len() * CHUNK_TAG_LEN
        );
    }

    #[test]
    /// `test_chunked_single_chunk` - test chunked single chunk.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn test_chunked_single_chunk() {
        let seed = [0x11u8; 32];
        let salt = [0x22u8; 32];
        let mut nonce = [0u8; NONCE_LEN];
        nonce[4..12].copy_from_slice(&[7, 8, 9, 10, 11, 12, 13, 14]);
        let plaintext = b"short";
        let chunk_sizes = vec![plaintext.len()];
        let ciphertext = encrypt_chunks(plaintext, &seed, &salt, &nonce, &chunk_sizes).unwrap();
        let decrypted = decrypt_chunks(&ciphertext, &seed, &salt, &nonce, &chunk_sizes).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    /// `test_chunked_wrong_key_fails` - test chunked wrong key fails.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn test_chunked_wrong_key_fails() {
        let seed1 = [0x01u8; 32];
        let seed2 = [0x02u8; 32];
        let salt = [0x03u8; 32];
        let mut nonce = [0u8; NONCE_LEN];
        nonce[4..12].copy_from_slice(&[4, 5, 6, 7, 8, 9, 10, 11]);
        let plaintext = b"secret chunked data";
        let chunk_sizes = vec![plaintext.len()];
        let ciphertext = encrypt_chunks(plaintext, &seed1, &salt, &nonce, &chunk_sizes).unwrap();
        let result = decrypt_chunks(&ciphertext, &seed2, &salt, &nonce, &chunk_sizes);
        assert!(result.is_err());
    }

    #[test]
    /// `test_chunked_corrupted_tag_fails` - test chunked corrupted tag fails.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn test_chunked_corrupted_tag_fails() {
        let seed = [0xAAu8; 32];
        let salt = [0xBBu8; 32];
        let mut nonce = [0u8; NONCE_LEN];
        nonce[4..12].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let plaintext = b"chunked auth test";
        let chunk_sizes = vec![plaintext.len()];
        let mut ciphertext = encrypt_chunks(plaintext, &seed, &salt, &nonce, &chunk_sizes).unwrap();
        // Corrupt the last byte of the tag
        if let Some(last) = ciphertext.last_mut() {
            *last ^= 0xFF;
        }
        let result = decrypt_chunks(&ciphertext, &seed, &salt, &nonce, &chunk_sizes);
        assert!(result.is_err());
    }
}
