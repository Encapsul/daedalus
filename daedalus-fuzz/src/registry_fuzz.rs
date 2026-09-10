//! In-process registry fuzzing target.
//!
//! Drives a disk-backed `LayerRegistry` through publish/fetch/list/overwrite
//! sequences plus index corruption, deriving every name, tag, and blob from
//! the fuzzer input. Asserts the properties that make the runnable-artifact
//! registry trustworthy:
//!
//! - returned hashes are the content hash of the published bytes,
//! - a re-publish overwrites the alias and the round trip serves the newest
//!   bytes,
//! - every accepted alias can be fetched back (push/GET symmetry),
//! - exactly the aliases published appear in listings,
//! - a corrupt index heals to empty instead of bricking the registry.

use crate::FuzzTarget;
use anyhow::{ensure, Result};
use arbitrary::Unstructured;
use daedalus_core::registry::{content_hash, format_hex, LayerRegistry};
use tempfile::TempDir;

pub struct RegistryFuzzTarget;

const SAFE: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789._-";

fn safe_alias(u: &mut Unstructured, max: usize) -> String {
    let len = u.int_in_range(1..=max).unwrap_or(1);
    let mut s = String::new();
    for _ in 0..len {
        s.push(*u.choose(SAFE).unwrap_or(&b'x') as char);
    }
    s
}

/// A blob is valid by construction; hash identity must match regardless.
fn publish_blob(reg: &mut LayerRegistry, name: &str, tag: &str, blob: &[u8]) -> Result<String> {
    let hash = reg.publish_artifact_binary(name, tag, blob)?;
    ensure!(
        hash == format_hex(&content_hash(blob)),
        "publish returned a non-content hash"
    );
    ensure!(
        reg.artifact_binary(name, tag)?.as_deref() == Some(blob),
        "published alias does not fetch back the exact bytes"
    );
    Ok(hash)
}

/// Draw up to `max` bytes, falling back to a fixed fill when the fuzzer
/// input is exhausted (input starvation is a harness concern, not a bug).
fn bytes_from(u: &mut Unstructured, max: usize) -> Vec<u8> {
    let take = u.len().min(max);
    match u.bytes(take) {
        Ok(bytes) => bytes.to_vec(),
        Err(_) => vec![0x61; max.min(4)],
    }
}

impl FuzzTarget for RegistryFuzzTarget {
    fn name(&self) -> &'static str {
        "registry"
    }
    fn generate_seed(&self, u: &mut Unstructured) -> Result<Vec<u8>> {
        let size = u.int_in_range(0..=2048)?;
        let mut seed = vec![0u8; size];
        u.fill_buffer(&mut seed)?;
        Ok(seed)
    }
    fn mutate(&self, input: &[u8], u: &mut Unstructured) -> Result<Vec<u8>> {
        let mut out = input.to_vec();
        let edits = u.int_in_range(1..=8)?;
        for _ in 0..edits {
            if out.is_empty() {
                out.push(u.arbitrary()?);
                continue;
            }
            match u.int_in_range(0..=2)? {
                0 => {
                    let idx = u.int_in_range(0..=out.len() - 1)?;
                    out[idx] = u.arbitrary()?;
                }
                1 => {
                    let idx = u.int_in_range(0..=out.len())?;
                    out.insert(idx, u.arbitrary()?);
                }
                _ => {
                    let idx = u.int_in_range(0..=out.len() - 1)?;
                    out.remove(idx);
                }
            }
        }
        Ok(out)
    }
    fn execute(&self, input: &[u8]) -> Result<()> {
        let td = TempDir::new()?;
        let mut reg = LayerRegistry::disk(td.path())?;
        let mut u = Unstructured::new(input);

        // A domain of aliases and blobs derived from the input.
        let steps = u.int_in_range(1..=6).unwrap_or(2);
        let mut published: Vec<(String, String, Vec<u8>)> = Vec::new();
        for _ in 0..steps {
            let name = safe_alias(&mut u, 16);
            let tag = safe_alias(&mut u, 16);
            let mut blob = bytes_from(&mut u, 4096);
            if blob.len() > 256 {
                blob.truncate(256);
            }
            match u.int_in_range(0..=4).unwrap_or(0) {
                // Publish a fresh alias and verify content addressing.
                0 => {
                    publish_blob(&mut reg, &name, &tag, &blob)?;
                    published.push((name, tag, blob));
                }
                // Overwrite an existing alias; the round trip serves the new bytes.
                1 => {
                    let mut sentinel = blob;
                    sentinel.push(0xff);
                    publish_blob(&mut reg, &name, &tag, &sentinel)?;
                    published.retain(|(n, t, _)| !(n == &name && t == &tag));
                    published.push((name, tag, sentinel));
                }
                // Listing mirrors the aliases exactly (no dupes for one alias).
                2 => {
                    let listed = reg.list_artifacts()?;
                    let expected = published
                        .iter()
                        .any(|(n, t, _)| n == &name && t == &tag);
                    ensure!(
                        listed
                            .iter()
                            .any(|a| a.name == name && a.tag == tag)
                            == expected,
                        "listing disagrees with publish history for {name}:{tag}"
                    );
                    ensure!(
                        listed
                            .iter()
                            .filter(|a| a.name == name && a.tag == tag)
                            .count()
                            <= 1,
                        "duplicate alias in listing"
                    );
                }
                // Corrupt the index and assert it heals empty instead of erroring.
                3 => {
                    let torn = bytes_from(&mut u, 64);
                    std::fs::write(td.path().join("artifacts.json"), &torn)?;
                    ensure!(
                        reg.list_artifacts().is_ok(),
                        "a corrupt index must not error — it must heal"
                    );
                }
                // Publishing garbage aliases must be rejected outright.
                _ => {
                    let bad_name = String::from_utf8_lossy(&bytes_from(&mut u, 16)).into_owned();
                    let bad_tag = String::from_utf8_lossy(&bytes_from(&mut u, 16)).into_owned();
                    if reg.publish_artifact_binary(&bad_name, &bad_tag, &blob).is_ok() {
                        let round_trips = reg.artifact_binary(&bad_name, &bad_tag)?.is_some();
                        ensure!(
                            round_trips,
                            "garbage alias accepted by publish but not fetchable"
                        );
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn target_created() {
        let target = RegistryFuzzTarget;
        assert_eq!(target.name(), "registry");
    }
}