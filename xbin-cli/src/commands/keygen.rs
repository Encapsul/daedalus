use anyhow::{Context, Result};
use clap::Args;
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use std::path::PathBuf;

#[derive(Args)]
pub struct KeygenArgs {
    /// Directory to write keys to
    #[arg(long, default_value = ".")]
    pub key_dir: PathBuf,

    /// Quiet output
    #[arg(short, long)]
    pub quiet: bool,
}

pub fn run(args: KeygenArgs) -> Result<()> {
    let key_dir = if args.key_dir == PathBuf::from(".") {
        default_key_dir()?
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

    std::fs::write(&key_path, signing_key.to_bytes())
        .with_context(|| format!("failed to write private key to {}", key_path.display()))?;
    std::fs::write(&pub_path, verifying_key.as_bytes())
        .with_context(|| format!("failed to write public key to {}", pub_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
    }

    if !args.quiet {
        eprintln!("Generated Ed25519 keypair");
        eprintln!("  fingerprint: {fingerprint}");
        eprintln!("  private key: {}", key_path.display());
        eprintln!("  public key:  {}", pub_path.display());
    }

    println!("{fingerprint}");

    Ok(())
}

fn default_key_dir() -> Result<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        Ok(PathBuf::from(xdg).join("xbin").join("keys"))
    } else if let Ok(home) = std::env::var("HOME") {
        Ok(PathBuf::from(home).join(".local").join("share").join("xbin").join("keys"))
    } else {
        Ok(PathBuf::from(".xbin").join("keys"))
    }
}
