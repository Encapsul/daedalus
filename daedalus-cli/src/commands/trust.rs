use anyhow::{Context, Result};
use clap::Args;
use daedalus_core::paths::trusted_keys_dir;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct TrustArgs {
    /// Path to the public key file to trust (omit with `--list`)
    pub pubkey: Option<PathBuf>,

    /// List fingerprints currently in the trusted keys directory
    #[arg(long)]
    pub list: bool,

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

    /// Disable all interactive prompts (for CI/scripts)
    #[arg(long, global = true)]
    pub no_input: bool,
}

/// run - trust a public key, or list the trust store.
pub fn run(args: TrustArgs) -> Result<()> {
    let trusted_dir = args.trusted_dir.unwrap_or_else(trusted_keys_dir);
    if args.list {
        return list_trusted(&trusted_dir, args.json, args.quiet);
    }
    let Some(pubkey) = args.pubkey.as_ref() else {
        anyhow::bail!("missing public key file - pass a .pub path, or use --list");
    };
    trust_pubkey(pubkey, &trusted_dir, args.dry_run, args.json, args.quiet)
}

fn trust_pubkey(
    pubkey: &Path,
    trusted_dir: &Path,
    dry_run: bool,
    json: bool,
    quiet: bool,
) -> Result<()> {
    let key_bytes = std::fs::read(pubkey)
        .with_context(|| format!("failed to read public key at {}", pubkey.display()))?;
    if key_bytes.len() != 32 {
        anyhow::bail!("public key must be 32 bytes, got {} bytes", key_bytes.len());
    }

    let fingerprint = fingerprint_hex(&key_bytes);
    let dest = trusted_dir.join(format!("{fingerprint}.pub"));

    if dry_run {
        return emit_trust_result(json, quiet, true, pubkey, &fingerprint, &dest);
    }

    std::fs::create_dir_all(trusted_dir).with_context(|| {
        format!(
            "failed to create trusted key directory {}",
            trusted_dir.display()
        )
    })?;
    std::fs::copy(pubkey, &dest)?;
    emit_trust_result(json, quiet, false, pubkey, &fingerprint, &dest)
}

fn emit_trust_result(
    json: bool,
    quiet: bool,
    dry_run: bool,
    pubkey: &Path,
    fingerprint: &str,
    dest: &Path,
) -> Result<()> {
    if json {
        let out = serde_json::json!({
            "dry_run": dry_run,
            "trusted": !dry_run,
            "pubkey": pubkey.to_string_lossy(),
            "fingerprint": fingerprint,
            "stored_at": dest.to_string_lossy(),
        });
        println!("{}", serde_json::to_string(&out)?);
        return Ok(());
    }
    if quiet {
        return Ok(());
    }
    if dry_run {
        eprintln!("Would trust key {}", pubkey.display());
    } else {
        eprintln!("Trusted key {}", pubkey.display());
    }
    eprintln!("  fingerprint: {fingerprint}");
    eprintln!("  stored at:   {}", dest.display());
    Ok(())
}

fn list_trusted(trusted_dir: &Path, json: bool, quiet: bool) -> Result<()> {
    let entries = collect_trusted_entries(trusted_dir)?;
    if json {
        let out = serde_json::json!({
            "trusted_dir": trusted_dir.to_string_lossy(),
            "count": entries.len(),
            "keys": entries.iter().map(|(fp, path)| serde_json::json!({
                "fingerprint": fp,
                "path": path.to_string_lossy(),
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    if quiet {
        return Ok(());
    }
    if entries.is_empty() {
        eprintln!("no trusted keys in {}", trusted_dir.display());
        eprintln!("add one with: daedalus trust <pubkey_file>");
        return Ok(());
    }
    eprintln!(
        "trusted keys in {} ({}):",
        trusted_dir.display(),
        entries.len()
    );
    for (fp, path) in &entries {
        println!("{fp}\t{}", path.display());
    }
    Ok(())
}

fn collect_trusted_entries(trusted_dir: &Path) -> Result<Vec<(String, PathBuf)>> {
    if !trusted_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(trusted_dir)
        .with_context(|| format!("failed to read {}", trusted_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("pub") {
            continue;
        }
        let bytes =
            std::fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
        if bytes.len() != 32 {
            continue;
        }
        let fp = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| fingerprint_hex(&bytes));
        entries.push((fp, path));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}

fn fingerprint_hex(key_bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key_bytes);
    hex::encode(hasher.finalize())
}
