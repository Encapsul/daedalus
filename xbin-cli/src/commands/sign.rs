use anyhow::{Context, Result};
use clap::Args;
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use xbin_core::format::{Footer, FLAG_SIGNED, SIG_BLOCK_SIZE};

#[derive(Args)]
pub struct SignArgs {
    /// Path to the .xbin file
    pub file: PathBuf,

    /// Path to the signing key file
    #[arg(short, long)]
    pub key: Option<PathBuf>,

    /// Quiet output
    #[arg(short, long)]
    pub quiet: bool,
}

pub fn run(args: SignArgs) -> Result<()> {
    let key_path = match args.key {
        Some(p) => p,
        None => {
            let dir = default_key_dir();
            let keys: Vec<_> = std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map_or(false, |ext| ext == "key"))
                .collect();
            if keys.len() == 1 {
                keys[0].path()
            } else {
                anyhow::bail!("[xbin] error: specify key with --key");
            }
        }
    };

    let key_bytes = std::fs::read(&key_path)
        .with_context(|| format!("failed to read signing key at {}", key_path.display()))?;
    if key_bytes.len() != 32 {
        anyhow::bail!(
            "[xbin] error: key must be 32 bytes, got {}",
            key_bytes.len()
        );
    }

    let mut key_arr = [0u8; 32];
    key_arr.copy_from_slice(&key_bytes);
    let signing_key = SigningKey::from_bytes(&key_arr);

    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&args.file)
        .with_context(|| format!("failed to open {}", args.file.display()))?;
    let mut footer = Footer::read_from(&mut f).context("failed to read xbin footer")?;

    if footer.is_signed() {
        anyhow::bail!("[xbin] error: file is already signed");
    }

    // Read payload and metadata for hash
    let payload =
        xbin_core::format::read_at(&mut f, footer.payload_offset, footer.payload_csize as usize)?;
    let meta = xbin_core::format::read_at(&mut f, footer.meta_offset, footer.meta_size as usize)?;

    // SHA-256(payload || meta)
    let mut hasher = Sha256::new();
    hasher.update(&payload);
    hasher.update(&meta);
    let hash = hasher.finalize();

    // Sign
    let signature = signing_key.sign(&hash);

    // Write sig_block: [sig_size:u32le][64-byte sig]
    let sig_size = SIG_BLOCK_SIZE as u32;
    let mut sig_block = Vec::with_capacity(SIG_BLOCK_SIZE as usize);
    sig_block.extend_from_slice(&sig_size.to_le_bytes());
    sig_block.extend_from_slice(&signature.to_bytes());

    let new_sig_offset = footer.meta_offset + footer.meta_size;
    let new_footer_offset = new_sig_offset + SIG_BLOCK_SIZE as u64;

    use std::io::{Seek, Write};

    // Write sig_block right after metadata
    f.seek(std::io::SeekFrom::Start(new_sig_offset))?;
    f.write_all(&sig_block)?;

    // Update footer: set sig_offset, flags, version
    footer.sig_offset = new_sig_offset;
    footer.flags |= FLAG_SIGNED;
    if footer.format_version < 3 {
        footer.format_version = 3;
    }

    // Write V3 footer (92 bytes) after sig_block: [sig_offset:u64le][core:84]
    let mut v3_footer = Vec::with_capacity(xbin_core::format::V3_FOOTER_SIZE as usize);
    v3_footer.extend_from_slice(&new_sig_offset.to_le_bytes());
    v3_footer.extend_from_slice(&footer.pack());
    f.seek(std::io::SeekFrom::Start(new_footer_offset))?;
    f.write_all(&v3_footer)?;

    // Truncate file to new size
    f.set_len(new_footer_offset + xbin_core::format::V3_FOOTER_SIZE)?;

    if !args.quiet {
        eprintln!("Signed {}", args.file.display());
        eprintln!("  key:    {}", key_path.display());
        eprintln!("  offset: {new_sig_offset}");
    }

    Ok(())
}

fn default_key_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(xdg).join("xbin").join("keys")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("xbin")
            .join("keys")
    } else {
        PathBuf::from(".xbin").join("keys")
    }
}
