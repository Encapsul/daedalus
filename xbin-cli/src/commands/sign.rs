use anyhow::{Context, Result};
use clap::Args;
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use xbin_core::format::{Footer, FLAG_SIGNED, SIG_BLOCK_SIZE, SIG_LEN};
use xbin_core::paths::default_key_dir;
use zeroize::Zeroizing;

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

    /// Force overwrite without confirmation
    #[arg(short, long)]
    pub force: bool,

    /// Output result as JSON
    #[arg(long)]
    pub json: bool,
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
                anyhow::bail!("specify key with --key");
            }
        }
    };

    if !args.force && !args.quiet {
        eprint!("Sign {}? [y/N] ", args.file.display());
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            eprintln!("Aborted");
            return Ok(());
        }
    }

    sign_file(&args.file, &key_path, args.quiet)?;

    if args.json {
        let info = serde_json::json!({
            "file": args.file.display().to_string(),
            "signed": true,
            "key": key_path.display().to_string(),
        });
        println!("{}", serde_json::to_string_pretty(&info)?);
    }

    Ok(())
}

/// Sign a `.xbin` file in-place with the given key. Used by both `xbin sign`
/// and `xbin build --key`.
///
/// Write is atomic: a temp file is created in the same directory and
/// renamed over the source only after the new content is fully flushed.
pub fn sign_file(file: &PathBuf, key_path: &PathBuf, quiet: bool) -> Result<()> {
    let key_bytes = Zeroizing::new(
        std::fs::read(key_path)
            .with_context(|| format!("failed to read signing key at {}", key_path.display()))?,
    );
    if key_bytes.len() != 32 {
        anyhow::bail!("key must be 32 bytes, got {}", key_bytes.len());
    }

    let mut key_arr = Zeroizing::new([0u8; 32]);
    key_arr.copy_from_slice(&key_bytes);
    let signing_key = SigningKey::from_bytes(&key_arr);

    let original =
        std::fs::read(file).with_context(|| format!("failed to read {}", file.display()))?;
    let mut cursor = std::io::Cursor::new(&original);
    let mut footer = Footer::read_from(&mut cursor).context("failed to read xbin footer")?;

    if footer.is_signed() {
        anyhow::bail!("file is already signed");
    }
    if footer.has_sisr() {
        // `--enable-sisr` already signs the delta manifest with `--key`;
        // inserting a binary sig block here would rebuild the file as
        // `[..meta_end][sig][footer]` and truncate the SISR section.
        anyhow::bail!(
            "cannot sign a SISR binary: the delta manifest is already signed; \
             rebuild without `--enable-sisr` to sign the whole binary"
        );
    }

    let meta_start = footer.meta_offset as usize;
    let meta_end = meta_start + footer.meta_size as usize;
    let payload_start = footer.payload_offset as usize;
    let payload_end = payload_start + footer.payload_csize as usize;

    let payload = original[payload_start..payload_end].to_vec();
    let meta = original[meta_start..meta_end].to_vec();

    // Mutate the footer to its final on-disk form FIRST: the digest covers
    // the footer itself (via `pack_full`, incl. the sig_offset prefix),
    // because the footer's format_version and FLAG_SIGNED decide whether the
    // signature is ever consulted. A signature over payload‖meta alone would
    // let an attacker downgrade the file to v2 and strip the flag — the
    // signature would be silently skipped.
    let new_sig_offset = footer.meta_offset + footer.meta_size;
    footer.sig_offset = new_sig_offset;
    footer.flags |= FLAG_SIGNED;
    if footer.format_version < 3 {
        footer.format_version = 3;
    }

    let mut hasher = Sha256::new();
    hasher.update(&payload);
    hasher.update(&meta);
    hasher.update(&footer.pack_full());
    let hash = hasher.finalize();

    let signature = signing_key.sign(&hash);

    let mut sig_block = Vec::with_capacity(SIG_BLOCK_SIZE);
    sig_block.extend_from_slice(&(SIG_LEN as u32).to_le_bytes());
    sig_block.extend_from_slice(&signature.to_bytes());

    let new_content: Vec<u8> = original[0..meta_end]
        .iter()
        .chain(sig_block.iter())
        .chain(footer.pack_full().iter())
        .copied()
        .collect();

    let tmp_path = file.with_extension("xbin.tmp");
    std::fs::write(&tmp_path, &new_content)
        .with_context(|| format!("failed to write temp file {}", tmp_path.display()))?;
    // `fs::write` creates the temp file with default perms; restore the
    // executable bit that `assemble` set, or signing would produce a binary
    // that the shell refuses to run.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp_path, file)
        .with_context(|| format!("failed to rename temp file to {}", file.display()))?;

    if !quiet {
        eprintln!("Signed {}", file.display());
        eprintln!("  key:    {}", key_path.display());
        eprintln!("  offset: {new_sig_offset}");
    }

    Ok(())
}
