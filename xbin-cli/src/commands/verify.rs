use anyhow::{Context, Result};
use clap::Args;
use ed25519_dalek::{Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use xbin_core::format::{Footer, SIG_BLOCK_SIZE_FIELD};

#[derive(Args)]
pub struct VerifyArgs {
    /// Path to the .xbin file
    pub file: PathBuf,

    /// Directory containing trusted public keys
    #[arg(long)]
    pub trusted_dir: Option<PathBuf>,

    /// Quiet output
    #[arg(short, long)]
    pub quiet: bool,
}

pub fn run(args: VerifyArgs) -> Result<()> {
    let trusted_dir = args.trusted_dir.unwrap_or_else(default_trusted_dir);

    let mut f = std::fs::File::open(&args.file)
        .with_context(|| format!("failed to open {}", args.file.display()))?;
    let footer = Footer::read_from(&mut f).context("failed to read xbin footer")?;

    if !footer.is_signed() {
        anyhow::bail!("[xbin] error: file is not signed");
    }

    // Read sig block
    let sig_data = xbin_core::format::read_at(
        &mut f,
        footer.sig_offset,
        SIG_BLOCK_SIZE_FIELD as usize + 64,
    )?;
    let sig_size = u32::from_le_bytes([sig_data[0], sig_data[1], sig_data[2], sig_data[3]]);
    let signature_bytes = &sig_data[4..4 + sig_size as usize];

    // Read payload and metadata for hash
    let payload =
        xbin_core::format::read_at(&mut f, footer.payload_offset, footer.payload_csize as usize)?;
    let meta = xbin_core::format::read_at(&mut f, footer.meta_offset, footer.meta_size as usize)?;

    // SHA-256(payload || meta)
    let mut hasher = Sha256::new();
    hasher.update(&payload);
    hasher.update(&meta);
    let hash = hasher.finalize();

    // Try each trusted key
    let keys = load_trusted_keys(&trusted_dir)?;
    if keys.is_empty() {
        anyhow::bail!(
            "[xbin] error: no trusted keys in {} — add keys with: xbin trust <pubkey_file>",
            trusted_dir.display()
        );
    }

    let sig = ed25519_dalek::Signature::from_slice(signature_bytes)?;
    let mut verified = false;

    for (key_path, vk) in &keys {
        if vk.verify(&hash, &sig).is_ok() {
            verified = true;
            if !args.quiet {
                eprintln!("Verified against {}", key_path.display());
            }
            break;
        }
    }

    if !verified {
        anyhow::bail!("[xbin] error: signature does not match any trusted key");
    }

    if !args.quiet {
        eprintln!("OK: signature verified");
    }

    Ok(())
}

fn load_trusted_keys(dir: &PathBuf) -> Result<Vec<(PathBuf, VerifyingKey)>> {
    let mut keys = Vec::new();
    if !dir.exists() {
        return Ok(keys);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "pub") {
            let key_bytes = std::fs::read(&path)?;
            if key_bytes.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&key_bytes);
                if let Ok(vk) = VerifyingKey::from_bytes(&arr) {
                    keys.push((path, vk));
                }
            }
        }
    }
    Ok(keys)
}

fn default_trusted_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(xdg).join("xbin").join("trusted-keys")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("xbin")
            .join("trusted-keys")
    } else {
        PathBuf::from(".xbin").join("trusted-keys")
    }
}
