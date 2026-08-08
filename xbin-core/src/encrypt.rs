use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;
use zeroize::{Zeroize, Zeroizing};

const NONCE_LEN: usize = 12;
const HKDF_SALT: &[u8] = b"xbin-encrypt-v1";
const HKDF_INFO: &[u8] = b"aes-256-gcm-key";

#[derive(Debug, Clone)]
pub struct EncryptMetadata {
    pub nonce: [u8; NONCE_LEN],
    pub tag_offset: usize,
}

pub fn hkdf_derive_key(signing_seed: &[u8; 32]) -> anyhow::Result<Zeroizing<[u8; 32]>> {
    let hk = Hkdf::<Sha256>::new(Some(HKDF_SALT), signing_seed);
    let mut key = Zeroizing::new([0u8; 32]);
    hk.expand(HKDF_INFO, &mut key[..])
        .map_err(|e| anyhow::anyhow!("HKDF expand failed: {e}"))?;
    Ok(key)
}

pub fn encrypt_payload(
    plaintext: &[u8],
    signing_seed: &[u8; 32],
) -> anyhow::Result<(Vec<u8>, EncryptMetadata)> {
    let key = hkdf_derive_key(signing_seed)?;
    let cipher =
        Aes256Gcm::new_from_slice(&key[..]).map_err(|e| anyhow::anyhow!("aes key init: {e}"))?;

    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from(nonce_bytes);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| anyhow::anyhow!("aes encrypt: {e}"))?;
    let metadata = EncryptMetadata {
        nonce: nonce_bytes,
        tag_offset: plaintext.len(),
    };
    // `Nonce::from`, metadata assignment, and `encrypt` all copied the nonce bytes;
    // scrub the local copy that is no longer needed.
    nonce_bytes.zeroize();

    Ok((ciphertext, metadata))
}

pub fn decrypt_payload(
    ciphertext: &[u8],
    signing_seed: &[u8; 32],
    nonce: &[u8; NONCE_LEN],
) -> anyhow::Result<Vec<u8>> {
    let key = hkdf_derive_key(signing_seed)?;
    let cipher =
        Aes256Gcm::new_from_slice(&key[..]).map_err(|e| anyhow::anyhow!("aes key init: {e}"))?;
    let nonce = Nonce::from(*nonce);

    let plaintext = cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("aes decrypt: {e}"))?;
    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hkdf_deterministic() {
        let seed = [0x42u8; 32];
        let key1 = hkdf_derive_key(&seed).unwrap();
        let key2 = hkdf_derive_key(&seed).unwrap();
        assert_eq!(key1, key2);
        assert_eq!(key1.len(), 32);
    }

    #[test]
    fn test_hkdf_wrong_length_panics() {
        let seed = vec![0u8; 16];
        let result: Result<&[u8; 32], _> = seed.as_slice().try_into();
        assert!(result.is_err());
    }

    #[test]
    fn test_roundtrip() {
        let seed = [0xABu8; 32];
        let plaintext = b"hello xbin encryption test data";
        let (ciphertext, meta) = encrypt_payload(plaintext, &seed).unwrap();

        assert_eq!(ciphertext.len(), plaintext.len() + 16);
        assert_eq!(meta.nonce.len(), NONCE_LEN);
        assert_eq!(meta.tag_offset, plaintext.len());

        let decrypted = decrypt_payload(&ciphertext, &seed, &meta.nonce).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_different_seeds_produce_different_ciphertext() {
        let seed1 = [0x01u8; 32];
        let seed2 = [0x02u8; 32];
        let plaintext = b"same plaintext both times";

        let (ct1, _) = encrypt_payload(plaintext, &seed1).unwrap();
        let (ct2, _) = encrypt_payload(plaintext, &seed2).unwrap();

        assert_ne!(ct1, ct2);
    }

    #[test]
    fn test_decrypt_wrong_key_fails() {
        let seed1 = [0x01u8; 32];
        let seed2 = [0x02u8; 32];
        let plaintext = b"secret data";

        let (ciphertext, meta) = encrypt_payload(plaintext, &seed1).unwrap();
        let result = decrypt_payload(&ciphertext, &seed2, &meta.nonce);

        assert!(result.is_err());
    }
}
