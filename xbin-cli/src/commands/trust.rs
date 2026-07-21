use anyhow::{Context, Result};
use clap::Args;
use sha2::{Sha256, Digest};
use std::path::PathBuf;

#[derive(Args)]
pub struct TrustArgs {
    /// Path to the public key file to trust
    pub pubkey: PathBuf,

    /// Directory to store trusted keys
    #[arg(long)]
    pub trusted_dir: Option<PathBuf>,

    /// Quiet output
    #[arg(short, long)]
    pub quiet: bool,
}

pub fn run(args: TrustArgs) -> Result<()> {
    let trusted_dir = args.trusted_dir.unwrap_or_else(default_trusted_dir);
    std::fs::create_dir_all(&trusted_dir)
        .with_context(|| format!("failed to create trusted key directory {}", trusted_dir.display()))?;

    let key_bytes = std::fs::read(&args.pubkey)
        .with_context(|| format!("failed to read public key at {}", args.pubkey.display()))?;
    if key_bytes.len() != 32 {
        anyhow::bail!("[xbin] error: public key must be 32 bytes, got {}", key_bytes.len());
    }

    // Compute fingerprint
    let mut hasher = Sha256::new();
    hasher.update(&key_bytes);
    let fingerprint = hex::encode(hasher.finalize());

    let dest = trusted_dir.join(format!("{fingerprint}.pub"));
    std::fs::copy(&args.pubkey, &dest)?;

    if !args.quiet {
        eprintln!("Trusted key {}", args.pubkey.display());
        eprintln!("  fingerprint: {fingerprint}");
        eprintln!("  stored at:   {}", dest.display());
    }

    Ok(())
}

fn default_trusted_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(xdg).join("xbin").join("trusted-keys")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".local").join("share").join("xbin").join("trusted-keys")
    } else {
        PathBuf::from(".xbin").join("trusted-keys")
    }
}
