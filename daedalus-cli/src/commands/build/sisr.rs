use anyhow::{Context, Result};
use daedalus_core::sisr_stage::SisrBuildConfig;
use ed25519_dalek::SigningKey;
use std::path::PathBuf;
use zeroize::Zeroizing;

use super::sign::warn_if_insecure_key_permissions;

/// Builds the SISR stage config from the CLI args. When `--key` is given the
/// same 32-byte Ed25519 key that would sign the binary instead signs the SISR
/// manifest; its bytes are never printed (only the path appears in warnings).
pub(crate) fn build_sisr_config(key_path: &Option<PathBuf>) -> Result<SisrBuildConfig> {
    let signing_key = match key_path {
        Some(path) => {
            warn_if_insecure_key_permissions(path);
            let key_bytes =
                Zeroizing::new(std::fs::read(path).with_context(|| {
                    format!("failed to read signing key at {}", path.display())
                })?);
            if key_bytes.len() != 32 {
                anyhow::bail!("key must be 32 bytes, got {}", key_bytes.len());
            }
            let mut key_arr = Zeroizing::new([0u8; 32]);
            key_arr.copy_from_slice(&key_bytes);
            Some(SigningKey::from_bytes(&key_arr))
        }
        None => None,
    };
    let chunk_target_size = 64 << 10;
    Ok(SisrBuildConfig {
        enabled: true,
        chunk_target_size,
        signing_key,
    })
}

/// Report SISR bandwidth savings by comparing old vs new chunk sets.
pub(crate) fn report_sisr_bandwidth(
    prev: &daedalus_core::sisr_stage::RemoteManifest,
    new: &daedalus_core::sisr_stage::RemoteManifest,
) {
    let new_total: u64 = new
        .manifest
        .chunks
        .iter()
        .map(|c| u64::from(c.length))
        .sum();
    if new_total == 0 {
        return;
    }
    let new_hashes: std::collections::HashSet<&[u8; 32]> =
        new.manifest.chunks.iter().map(|c| &c.hash).collect();
    let mut reused: u64 = 0;
    for c in &prev.manifest.chunks {
        if new_hashes.contains(&c.hash) {
            reused += u64::from(c.length);
        }
    }
    let delta = new_total.saturating_sub(reused);
    if delta == 0 {
        eprintln!(
            "  SISR: no changes — 0 B delta vs {:.1} MB full (100% bandwidth saved)",
            new_total as f64 / (1024.0 * 1024.0)
        );
    } else {
        let pct = 100.0 - (delta as f64 / new_total as f64) * 100.0;
        eprintln!(
            "  SISR delta: {:.1} MB vs {:.1} MB full — {:.1}% bandwidth saved",
            delta as f64 / (1024.0 * 1024.0),
            new_total as f64 / (1024.0 * 1024.0),
            pct
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_sisr_config_defaults_no_key() {
        let cfg = build_sisr_config(&None).unwrap();
        assert!(cfg.enabled);
        assert_eq!(cfg.chunk_target_size, 64 << 10);
        assert!(cfg.signing_key.is_none());
    }

    #[test]
    fn build_sisr_config_loads_32_byte_key() {
        let dir = tempfile::TempDir::new().unwrap();
        let key_path = dir.path().join("key.bin");
        std::fs::write(&key_path, [42u8; 32]).unwrap();
        let cfg = build_sisr_config(&Some(key_path)).unwrap();
        assert!(cfg.signing_key.is_some());
    }

    #[test]
    fn build_sisr_config_rejects_wrong_key_length() {
        let dir = tempfile::TempDir::new().unwrap();
        let key_path = dir.path().join("key.bin");
        std::fs::write(&key_path, [42u8; 16]).unwrap();
        assert!(build_sisr_config(&Some(key_path)).is_err());
    }

    #[test]
    fn build_sisr_config_errors_on_missing_key() {
        let path = PathBuf::from("/nonexistent/key.bin");
        assert!(build_sisr_config(&Some(path)).is_err());
    }
}
