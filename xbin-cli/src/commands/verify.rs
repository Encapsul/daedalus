use anyhow::{Context, Result};
use clap::Args;
use ed25519_dalek::{Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use xbin_core::format::{Footer, SIG_BLOCK_SIZE_FIELD};
use xbin_core::paths::trusted_keys_dir;

fn write_json_output(value: &serde_json::Value, output: Option<&std::path::Path>) -> Result<()> {
    let json_str = serde_json::to_string_pretty(value)?;
    if let Some(path) = output {
        std::fs::write(path, &json_str)
            .with_context(|| format!("failed to write to {}", path.display()))?;
        eprintln!("Wrote JSON to {}", path.display());
    } else {
        println!("{json_str}");
    }
    Ok(())
}

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

    /// Output verification result as JSON
    #[arg(long)]
    pub json: bool,

    /// Write JSON output to file (requires --json)
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

pub fn run(args: VerifyArgs) -> Result<()> {
    let trusted_dir = args.trusted_dir.unwrap_or_else(trusted_keys_dir);

    let mut f = std::fs::File::open(&args.file)
        .with_context(|| format!("failed to open {}", args.file.display()))?;
    let footer = Footer::read_from(&mut f).context("failed to read xbin footer")?;

    if !footer.is_signed() {
        if args.json {
            let info = serde_json::json!({
                "file": args.file.display().to_string(),
                "signed": false,
            });
            write_json_output(&info, args.output.as_deref())?;
        } else {
            eprintln!("file is not signed");
        }
        return Ok(());
    }

    // Read sig block: [sig_size:u32le][64-byte ed25519 signature]
    let sig_data = xbin_core::format::read_at(
        &mut f,
        footer.sig_offset,
        SIG_BLOCK_SIZE_FIELD as usize + 64,
    )?;
    let sig_size = u32::from_le_bytes([sig_data[0], sig_data[1], sig_data[2], sig_data[3]]);
    if sig_size != 64 {
        anyhow::bail!("unexpected sig_size {} (expected 64 for ed25519)", sig_size);
    }
    let signature_bytes = &sig_data[4..68];

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
            "no trusted keys in {} — add keys with: xbin trust <pubkey_file>",
            trusted_dir.display()
        );
    }

    let sig = ed25519_dalek::Signature::from_slice(signature_bytes)?;
    let mut verified = false;
    let mut verified_key: Option<String> = None;

    for (key_path, vk) in &keys {
        if vk.verify(&hash, &sig).is_ok() {
            verified = true;
            verified_key = Some(key_path.display().to_string());
            if !args.quiet {
                eprintln!("Verified against {}", key_path.display());
            }
            break;
        }
    }

    if args.json {
        let info = serde_json::json!({
            "file": args.file.display().to_string(),
            "signed": true,
            "verified": verified,
            "key": verified_key,
        });
        write_json_output(&info, args.output.as_deref())?;
    } else if !verified {
        anyhow::bail!("signature does not match any trusted key");
    } else if !args.quiet {
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
