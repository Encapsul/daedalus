//! SISR manifest fuzzing target.
//!
//! Structure-aware fuzzing for SISR binary parsers: DeltaManifest, SisrFooterExt,
//! RemoteManifest, Footer, and read_sisr.

use crate::{FuzzTarget, MutationStrategy};
use anyhow::Result;
use arbitrary::Unstructured;
use std::io::Cursor;

pub struct SisrManifestFuzzTarget;

impl FuzzTarget for SisrManifestFuzzTarget {
    fn name(&self) -> &'static str {
        "sisr"
    }
    fn generate_seed(&self, unstructured: &mut Unstructured) -> Result<Vec<u8>> {
        let seed: [u8; 32] = unstructured.arbitrary()?;
        Ok(seed.to_vec())
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
                if out.len() > 100 && unstructured.ratio(1, 2)? {
                    let idx = unstructured.int_in_range(0..=out.len() - 100)?;
                    let len = unstructured.int_in_range(1..=100.min(out.len() - idx))?;
                    out.drain(idx..idx + len);
                } else if out.len() < 10_000 {
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
        let _ = daedalus_core::manifest::DeltaManifest::parse(input);
        let _ = daedalus_core::sisr_header::SisrFooterExt::parse(input);
        let _ = daedalus_core::sisr_stage::RemoteManifest::from_bytes(input);
        let _ = daedalus_core::format::Footer::read_from(&mut Cursor::new(input));
        let _ = daedalus_core::sisr_header::read_sisr(&mut Cursor::new(input));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_sisr_target_creation() {
        let target = SisrManifestFuzzTarget;
        assert_eq!(target.name(), "sisr");
    }
}
