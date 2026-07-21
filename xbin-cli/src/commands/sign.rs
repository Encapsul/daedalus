use clap::Args;
use ed25519_dalek::{SigningKey, Signer};
use sha2::{Sha256, Digest};
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

pub fn run(args: SignArgs) -> Result<(), Box<dyn std::error::Error>> {
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
                return Err("[xbin] error: specify key with --key".into());
            }
        }
    };

    let key_bytes = std::fs::read(&key_path)?;
    if key_bytes.len() != 32 {
        return Err(format!("[xbin] error: key must be 32 bytes, got {}", key_bytes.len()).into());
    }

    let mut key_arr = [0u8; 32];
    key_arr.copy_from_slice(&key_bytes);
    let signing_key = SigningKey::from_bytes(&key_arr);

    let mut f = std::fs::File::open(&args.file)?;
    let mut footer = Footer::read_from(&mut f)?;

    if footer.is_signed() {
        return Err("[xbin] error: file is already signed".into());
    }

    // Read payload and metadata for hash
    let payload = xbin_core::format::read_at(&mut f, footer.payload_offset, footer.payload_csize as usize)?;
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

    // Update footer
    let old_footer_size = footer.footer_size();
    let new_sig_offset = footer.meta_offset + footer.meta_size;

    // Write sig_block after metadata
    use std::io::{Seek, Write};
    f.seek(std::io::SeekFrom::Start(new_sig_offset))?;
    f.write_all(&sig_block)?;

    // Update footer: set sig_offset, flags, version
    footer.sig_offset = new_sig_offset;
    footer.flags |= FLAG_SIGNED;
    if footer.format_version < 3 {
        footer.format_version = 3;
    }

    // Write new footer at end
    f.seek(std::io::SeekFrom::End(-(old_footer_size as i64)))?;
    f.write_all(&footer.pack())?;

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
        PathBuf::from(home).join(".local").join("share").join("xbin").join("keys")
    } else {
        PathBuf::from(".xbin").join("keys")
    }
}
