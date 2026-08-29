//! Registry server fuzzing target.

use crate::FuzzTarget;
use anyhow::Result;
use arbitrary::Unstructured;

pub struct RegistryFuzzTarget;

impl FuzzTarget for RegistryFuzzTarget {
    fn name(&self) -> &'static str {
        "registry"
    }
    fn generate_seed(&self, unstructured: &mut Unstructured) -> Result<Vec<u8>> {
        let action = unstructured.choose(&["push", "pull", "list", "read"])?;
        let name_len = unstructured.int_in_range(1..=32)?;
        let name: String = (0..name_len)
            .map(|_| unstructured.arbitrary::<char>().unwrap_or('x'))
            .collect();
        let seed = format!("{action}:{name}");
        Ok(seed.into_bytes())
    }
    fn mutate(&self, input: &[u8], unstructured: &mut Unstructured) -> Result<Vec<u8>> {
        let mut out = input.to_vec();
        if !out.is_empty() && unstructured.ratio(1, 2)? {
            let idx = unstructured.int_in_range(0..=out.len() - 1)?;
            out[idx] = unstructured.arbitrary::<u8>()?;
        } else if out.len() < 1024 {
            let byte: u8 = unstructured.arbitrary()?;
            out.push(byte);
        }
        Ok(out)
    }
    fn execute(&self, input: &[u8]) -> Result<()> {
        let _input = input;
        // Registry operations require a running server; skip in fuzzing
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_registry_target_creation() {
        let target = RegistryFuzzTarget;
        assert_eq!(target.name(), "registry");
    }
}
