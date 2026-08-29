//! Serve/chat inference pipeline fuzzing target.

use crate::{FuzzTarget, MutationStrategy};
use anyhow::Result;
use arbitrary::Unstructured;

pub struct ServeFuzzTarget;

impl FuzzTarget for ServeFuzzTarget {
    fn name(&self) -> &'static str {
        "serve"
    }
    fn generate_seed(&self, unstructured: &mut Unstructured) -> Result<Vec<u8>> {
        let endpoint =
            unstructured.choose(&["/healthz", "/readyz", "/v1/chat", "/v1/models", "/metrics"])?;
        let payload_size = unstructured.int_in_range(0..=512)?;
        let payload: Vec<u8> = (0..payload_size)
            .map(|_| unstructured.arbitrary::<u8>().unwrap_or(0))
            .collect();
        let mut seed = Vec::new();
        seed.extend_from_slice(endpoint.as_bytes());
        seed.push(b'\0');
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
                for _ in 0..unstructured.int_in_range(1..=4)? {
                    if out.is_empty() {
                        break;
                    }
                    let idx = unstructured.int_in_range(0..=out.len().saturating_sub(1))?;
                    out[idx] ^= 1 << unstructured.int_in_range(0..=7)?;
                }
            }
            MutationStrategy::ByteInsertDelete => {
                if out.len() > 50 && unstructured.ratio(1, 2)? {
                    let idx = unstructured.int_in_range(0..=out.len() - 50)?;
                    let len = unstructured.int_in_range(1..=50.min(out.len() - idx))?;
                    out.drain(idx..idx + len);
                } else if out.len() < 5000 {
                    let idx = unstructured.int_in_range(0..=out.len())?;
                    let len = unstructured.int_in_range(1..=32)?;
                    let bytes: Vec<u8> = (0..len)
                        .map(|_| unstructured.arbitrary::<u8>().unwrap_or(0))
                        .collect();
                    out.splice(idx..idx, bytes);
                }
            }
            _ => {}
        }
        Ok(out)
    }
    fn execute(&self, input: &[u8]) -> Result<()> {
        let _input = input;
        // Serve requires a running server; skip in fuzzing
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_serve_target_creation() {
        let target = ServeFuzzTarget;
        assert_eq!(target.name(), "serve");
    }
}
