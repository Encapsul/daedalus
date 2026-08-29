//! Encryption/decryption fuzzing target for daedalus.
//!
//! Tests the AES-256-GCM encryption path with structure-aware mutations.

use crate::{FuzzTarget, MutationStrategy};
use anyhow::Result;
use arbitrary::Unstructured;

pub struct CryptoFuzzTarget;

impl FuzzTarget for CryptoFuzzTarget {
    fn name(&self) -> &'static str {
        "crypto"
    }
    fn generate_seed(&self, unstructured: &mut Unstructured) -> Result<Vec<u8>> {
        let payload_size = unstructured.int_in_range(0..=4096)?;
        let payload: Vec<u8> = (0..payload_size)
            .map(|_| unstructured.arbitrary().unwrap_or(0))
            .collect();
        let key: [u8; 32] = unstructured.arbitrary()?;
        let mut seed = Vec::new();
        seed.extend_from_slice(&key);
        seed.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        seed.extend_from_slice(&payload);
        Ok(seed)
    }
    fn mutate(&self, input: &[u8], unstructured: &mut Unstructured) -> Result<Vec<u8>> {
        let mut out = input.to_vec();
        let strategy = unstructured.choose(&[
            MutationStrategy::BitFlip,
            MutationStrategy::ByteInsertDelete,
        ])?;
        match strategy {
            MutationStrategy::BitFlip => {
                for _ in 0..unstructured.int_in_range(1..=8)? {
                    if out.is_empty() {
                        break;
                    }
                    let idx = unstructured.int_in_range(0..=out.len().saturating_sub(1))?;
                    out[idx] ^= 1 << unstructured.int_in_range(0..=7)?;
                }
            }
            MutationStrategy::ByteInsertDelete => {
                if out.len() > 64 && unstructured.ratio(1, 2)? {
                    let idx = unstructured.int_in_range(0..=out.len() - 64)?;
                    let len = unstructured.int_in_range(1..=64.min(out.len() - idx))?;
                    out.drain(idx..idx + len);
                } else if out.len() < 5000 {
                    let idx = unstructured.int_in_range(0..=out.len())?;
                    let len = unstructured.int_in_range(1..=64)?;
                    let bytes: Vec<u8> = (0..len)
                        .map(|_| unstructured.arbitrary().unwrap_or(0))
                        .collect();
                    out.splice(idx..idx, bytes);
                }
            }
            _ => {}
        }
        Ok(out)
    }
    fn execute(&self, input: &[u8]) -> Result<()> {
        if input.len() < 32 + 4 {
            return Ok(());
        }
        let key: [u8; 32] = input[..32].try_into().unwrap();
        let payload_len = u32::from_le_bytes(input[32..36].try_into().unwrap()) as usize;
        if input.len() < 36 + payload_len {
            return Ok(());
        }
        let payload = &input[36..36 + payload_len];
        // Try to encrypt then decrypt — should roundtrip
        let (ciphertext, meta) = daedalus_core::encrypt::encrypt_payload(payload, &key)
            .map_err(|e| anyhow::anyhow!("encrypt failed: {}", e))?;
        let _decrypted =
            daedalus_core::encrypt::decrypt_payload(&ciphertext, &key, &meta.nonce, &meta.salt)
                .map_err(|e| anyhow::anyhow!("decrypt failed: {}", e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_crypto_target_creation() {
        let target = CryptoFuzzTarget;
        assert_eq!(target.name(), "crypto");
    }
}
