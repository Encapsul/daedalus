//! Stub launcher fuzzing target.

use crate::{FuzzTarget, MutationStrategy};
use anyhow::Result;
use arbitrary::{Arbitrary, Unstructured};

pub struct StubFuzzTarget;

impl StubFuzzTarget {
    fn mutate_extraction(&self, input: &[u8], unstructured: &mut Unstructured) -> Result<Vec<u8>> {
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
                        .map(|_| unstructured.arbitrary().unwrap_or(0))
                        .collect();
                    out.splice(idx..idx, bytes);
                }
            }
            _ => {}
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, Arbitrary, serde::Serialize, serde::Deserialize)]
struct ExtractionScenario {
    payload_type: PayloadType,
    entrypoint: String,
}

#[derive(Debug, Clone, Arbitrary, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum PayloadType {
    Python,
    Node,
    GoBinary,
    RustBinary,
    Script,
    Mixed,
}

impl FuzzTarget for StubFuzzTarget {
    fn name(&self) -> &'static str {
        "stub"
    }
    fn generate_seed(&self, unstructured: &mut Unstructured) -> Result<Vec<u8>> {
        let scenario = ExtractionScenario::arbitrary(unstructured)?;
        Ok(serde_json::to_vec(&scenario)?)
    }
    fn mutate(&self, input: &[u8], unstructured: &mut Unstructured) -> Result<Vec<u8>> {
        self.mutate_extraction(input, unstructured)
    }
    fn execute(&self, input: &[u8]) -> Result<()> {
        let _scenario: ExtractionScenario = serde_json::from_slice(input)?;
        // Execution would run stub — skip in fuzzing
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_stub_target_creation() {
        let target = StubFuzzTarget;
        assert_eq!(target.name(), "stub");
    }
}
