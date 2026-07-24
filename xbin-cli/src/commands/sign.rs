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

    sign_file(&args.file, &key_path, args.quiet)
}

/// Sign a `.xbin` file in-place with the given key. Used by both `xbin sign`
/// and `xbin build --key`.
///
/// Write is atomic: a temp file is created in the same directory and
/// renamed over the source only after the new content is fully flushed.
pub fn sign_file(file: &PathBuf, key_path: &PathBuf, quiet: bool) -> Result<()> {
    let key_bytes = std::fs::read(key_path)
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

    let original =
        std::fs::read(file).with_context(|| format!("failed to read {}", file.display()))?;
    let mut cursor = std::io::Cursor::new(&original);
    let mut footer = Footer::read_from(&mut cursor).context("failed to read xbin footer")?;

    if footer.is_signed() {
        anyhow::bail!("[xbin] error: file is already signed");
    }

    let meta_start = footer.meta_offset as usize;
    let meta_end = meta_start + footer.meta_size as usize;
    let payload_start = footer.payload_offset as usize;
    let payload_end = payload_start + footer.payload_csize as usize;

    let payload = original[payload_start..payload_end].to_vec();
    let meta = original[meta_start..meta_end].to_vec();

    let mut hasher = Sha256::new();
    hasher.update(&payload);
    hasher.update(&meta);
    let hash = hasher.finalize();

    let signature = signing_key.sign(&hash);

    let sig_size = 64u32;
    let mut sig_block = Vec::with_capacity(SIG_BLOCK_SIZE as usize);
    sig_block.extend_from_slice(&sig_size.to_le_bytes());
    sig_block.extend_from_slice(&signature.to_bytes());

    let new_sig_offset = footer.meta_offset + footer.meta_size;

    footer.sig_offset = new_sig_offset;
    footer.flags |= FLAG_SIGNED;
    if footer.format_version < 3 {
        footer.format_version = 3;
    }

    let mut v3_footer = Vec::with_capacity(xbin_core::format::V3_FOOTER_SIZE as usize);
    v3_footer.extend_from_slice(&new_sig_offset.to_le_bytes());
    v3_footer.extend_from_slice(&footer.pack());

    let new_content: Vec<u8> = original[0..meta_end]
        .iter()
        .chain(sig_block.iter())
        .chain(v3_footer.iter())
        .copied()
        .collect();

    let tmp_path = file.with_extension("xbin.tmp");
    std::fs::write(&tmp_path, &new_content)
        .with_context(|| format!("failed to write temp file {}", tmp_path.display()))?;
    std::fs::rename(&tmp_path, file)
        .with_context(|| format!("failed to rename temp file to {}", file.display()))?;

    if !quiet {
        eprintln!("Signed {}", file.display());
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
