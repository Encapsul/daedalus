use anyhow::{Context, Result};
use clap::Args;
use daedalus_core::format::{Footer, SIG_BLOCK_SIZE, SIG_LEN};
use daedalus_core::paths::trusted_keys_dir;
use ed25519_dalek::{Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// write_json_output - serialize a serde_json Value to a file or stdout.
/// @value: value
/// @output: output destination
///
/// Description:
/// Pretty-prints the JSON value to the given file path or to stdout.
///
/// Return: Result containing Result<()>
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
    /// Path to the .daedalus file
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

    /// Disable all interactive prompts (for CI/scripts)
    #[arg(long, global = true)]
    pub no_input: bool,
}

/// run - verify the Ed25519 signature and SHA-256 integrity of a .daedalus file.
/// @args: command arguments
///
/// Description:
/// Reads the signature block, hashes payload || meta || footer, and checks
/// against all trusted public keys. Reports the verifying key path on success.
///
/// Return: Result containing Result<()>
pub fn run(args: VerifyArgs) -> Result<()> {
    let trusted_dir = args.trusted_dir.unwrap_or_else(trusted_keys_dir);

    let mut f = std::fs::File::open(&args.file)
        .with_context(|| format!("failed to open {}", args.file.display()))?;
    let footer = Footer::read_from(&mut f).context("failed to read daedalus footer")?;

    let has_sig_block = footer.format_version >= 3 && footer.sig_offset != 0;
    if has_sig_block != footer.is_signed() {
        anyhow::bail!("inconsistent signature state (flag/offset mismatch)");
    }
    if !has_sig_block {
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
    let sig_data = daedalus_core::format::read_at(&mut f, footer.sig_offset, SIG_BLOCK_SIZE)?;
    let sig_size = u32::from_le_bytes([sig_data[0], sig_data[1], sig_data[2], sig_data[3]]);
    if sig_size != SIG_LEN as u32 {
        anyhow::bail!(
            "unexpected sig_size {} (expected {SIG_LEN} for ed25519)",
            sig_size
        );
    }
    let signature_bytes = &sig_data[4..SIG_BLOCK_SIZE];

    // Read payload and metadata for hash
    let payload = daedalus_core::format::read_at(
        &mut f,
        footer.payload_offset,
        footer.payload_csize as usize,
    )?;
    let meta =
        daedalus_core::format::read_at(&mut f, footer.meta_offset, footer.meta_size as usize)?;

    // SHA-256(payload || meta || footer) — the footer is hashed so a downgrade
    // of format_version/FLAG_SIGNED invalidates the signature instead of
    // being silently skipped.
    let mut hasher = Sha256::new();
    hasher.update(&payload);
    hasher.update(&meta);
    hasher.update(footer.pack_full());
    let hash = hasher.finalize();

    // Try each trusted key
    let keys = load_trusted_keys(&trusted_dir)?;
    if keys.is_empty() {
        anyhow::bail!(
            "no trusted keys in {} — add keys with: daedalus trust <pubkey_file>",
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

/// load_trusted_keys - load all trusted Ed25519 public keys from a directory.
/// @dir: directory path
///
/// Description:
/// Reads every .pub file in dir, filters to exactly 32-byte entries, and
/// attempts to parse each as an Ed25519 VerifyingKey.
///
/// Return: Result containing Result<Vec<(PathBuf, VerifyingKey)>>
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
