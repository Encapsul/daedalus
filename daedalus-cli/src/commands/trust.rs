use anyhow::{Context, Result};
use clap::Args;
use daedalus_core::paths::trusted_keys_dir;
use sha2::{Digest, Sha256};
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

    /// Show what would be done without doing it
    #[arg(long)]
    pub dry_run: bool,

    /// Output result as JSON
    #[arg(long)]
    pub json: bool,
}

/// run - run.
/// @args: command arguments
///
/// Description:
///
/// Return: Result containing Result<()>
pub fn run(args: TrustArgs) -> Result<()> {
    let trusted_dir = args.trusted_dir.unwrap_or_else(trusted_keys_dir);

    let key_bytes = std::fs::read(&args.pubkey)
        .with_context(|| format!("failed to read public key at {}", args.pubkey.display()))?;
    if key_bytes.len() != 32 {
        anyhow::bail!("public key must be 32 bytes, got {} bytes", key_bytes.len());
    }

    let mut hasher = Sha256::new();
    hasher.update(&key_bytes);
    let fingerprint = hex::encode(hasher.finalize());

    let dest = trusted_dir.join(format!("{fingerprint}.pub"));

    if args.dry_run {
        if args.json {
            let out = serde_json::json!({
                "dry_run": true,
                "pubkey": args.pubkey.to_string_lossy().to_string(),
                "fingerprint": fingerprint,
                "stored_at": dest.to_string_lossy().to_string(),
            });
            println!("{}", serde_json::to_string(&out)?);
            return Ok(());
        }
        eprintln!("Would trust key {}", args.pubkey.display());
        eprintln!("  fingerprint: {fingerprint}");
        eprintln!("  stored at:   {}", dest.display());
        return Ok(());
    }

    std::fs::create_dir_all(&trusted_dir).with_context(|| {
        format!(
            "failed to create trusted key directory {}",
            trusted_dir.display()
        )
    })?;
    std::fs::copy(&args.pubkey, &dest)?;

    if args.json {
        let out = serde_json::json!({
            "trusted": true,
            "pubkey": args.pubkey.to_string_lossy().to_string(),
            "fingerprint": fingerprint,
            "stored_at": dest.to_string_lossy().to_string(),
        });
        println!("{}", serde_json::to_string(&out)?);
        return Ok(());
    }

    if !args.quiet {
        eprintln!("Trusted key {}", args.pubkey.display());
        eprintln!("  fingerprint: {fingerprint}");
        eprintln!("  stored at:   {}", dest.display());
    }

    Ok(())
}
