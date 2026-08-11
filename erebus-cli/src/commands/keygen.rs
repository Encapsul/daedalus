use anyhow::{Context, Result};
use clap::Args;
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use std::path::PathBuf;
use erebus_core::paths::default_key_dir;
use zeroize::Zeroize;

#[derive(Args)]
pub struct KeygenArgs {
    /// Directory to write keys to
    #[arg(long, default_value = ".")]
    pub key_dir: PathBuf,

    /// Quiet output
    #[arg(short, long)]
    pub quiet: bool,

    /// Force overwrite existing keys
    #[arg(short, long)]
    pub force: bool,

    /// Output result as JSON
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: KeygenArgs) -> Result<()> {
    let key_dir = if args.key_dir == PathBuf::from(".") {
        default_key_dir()
    } else {
        args.key_dir.clone()
    };

    std::fs::create_dir_all(&key_dir)
        .with_context(|| format!("failed to create key directory {}", key_dir.display()))?;

    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();

    let fingerprint = hex::encode(verifying_key.as_bytes());

    let key_path = key_dir.join(format!("{fingerprint}.key"));
    let pub_path = key_dir.join(format!("{fingerprint}.pub"));

    if key_path.exists() || pub_path.exists() {
        if args.force {
            std::fs::remove_file(&key_path).ok();
            std::fs::remove_file(&pub_path).ok();
        } else {
            anyhow::bail!(
                "key pair already exists at {}. Use --force to overwrite",
                key_dir.display()
            );
        }
    }

    let mut key_bytes = signing_key.to_bytes();
    std::fs::write(&key_path, &key_bytes)
        .with_context(|| format!("failed to write private key to {}", key_path.display()))?;
    key_bytes.zeroize();
    std::fs::write(&pub_path, verifying_key.as_bytes())
        .with_context(|| format!("failed to write public key to {}", pub_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
    }

    if args.json {
        let info = serde_json::json!({
            "fingerprint": fingerprint,
            "private_key": key_path.display().to_string(),
            "public_key": pub_path.display().to_string(),
        });
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else if !args.quiet {
        eprintln!("Generated Ed25519 keypair");
        eprintln!("  fingerprint: {fingerprint}");
        eprintln!("  private key: {}", key_path.display());
        eprintln!("  public key:  {}", pub_path.display());
    }

    Ok(())
}
