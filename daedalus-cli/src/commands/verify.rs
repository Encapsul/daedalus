use anyhow::{Context, Result};
use clap::Args;
use daedalus_core::format::{signature_digest, Footer, SIG_BLOCK_SIZE, SIG_LEN};
use daedalus_core::paths::trusted_keys_dir;
use daedalus_core::sisr_header::read_sisr;
use daedalus_core::sisr_stage::verify_embedded_sisr_bound;
use ed25519_dalek::VerifyingKey;
use std::io::Cursor;
use std::path::{Path, PathBuf};

/// write_json_output - serialize a serde_json Value to a file or stdout.
fn write_json_output(value: &serde_json::Value, output: Option<&Path>) -> Result<()> {
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
    /// Path to the .de file, or `-` to read from stdin
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

/// Verify trailing Ed25519 (v3+) or embedded SISR manifest signature.
pub fn run(args: VerifyArgs) -> Result<()> {
    let trusted_dir = args.trusted_dir.clone().unwrap_or_else(trusted_keys_dir);
    let display_name = display_name_of(&args.file);
    let mut source = open_source(&args.file)?;
    let footer = Footer::read_from(&mut source).context("failed to read daedalus footer")?;
    let has_sig_block = footer.format_version >= 3 && footer.sig_offset != 0;
    if has_sig_block != footer.is_signed() {
        anyhow::bail!("inconsistent signature state (flag/offset mismatch)");
    }
    if has_sig_block {
        return verify_trailing(&args, &display_name, &mut source, &footer, &trusted_dir);
    }
    if let Some((ext, manifest)) = read_sisr(&mut source).context("failed to read SISR section")? {
        if ext.signature != [0u8; 64] {
            return verify_sisr(
                &args,
                &display_name,
                &mut source,
                &footer,
                &ext,
                &manifest,
                &trusted_dir,
            );
        }
    }
    report_unsigned(&args, &display_name)
}

fn display_name_of(file: &Path) -> String {
    if crate::stdio::is_dash(&file.to_string_lossy()) {
        "stdin".to_string()
    } else {
        file.display().to_string()
    }
}

fn open_source(file: &Path) -> Result<Cursor<Vec<u8>>> {
    if crate::stdio::is_dash(&file.to_string_lossy()) {
        crate::stdio::read_stdin_cursor()
    } else {
        let bytes =
            std::fs::read(file).with_context(|| format!("failed to open {}", file.display()))?;
        Ok(Cursor::new(bytes))
    }
}

fn report_unsigned(args: &VerifyArgs, display_name: &str) -> Result<()> {
    if args.json {
        write_json_output(
            &serde_json::json!({
                "file": display_name,
                "signed": false,
                "verified": false,
                "error": "file is not signed",
            }),
            args.output.as_deref(),
        )?;
    }
    // Fail closed: CI gates and `f && ./f` must not treat an unsigned file
    // as verified. The JSON report is still emitted before the error.
    anyhow::bail!("file is not signed")
}

fn require_trusted_keys(trusted_dir: &Path) -> Result<Vec<(PathBuf, VerifyingKey)>> {
    let keys = load_trusted_keys(&trusted_dir.to_path_buf())?;
    if keys.is_empty() {
        anyhow::bail!(
            "no trusted keys in {} - recipient machines start empty; \
             add the publisher .pub with: daedalus trust <pubkey_file> \
             (inspect with: daedalus trust --list)",
            trusted_dir.display()
        );
    }
    Ok(keys)
}

fn verify_trailing(
    args: &VerifyArgs,
    display_name: &str,
    source: &mut Cursor<Vec<u8>>,
    footer: &Footer,
    trusted_dir: &Path,
) -> Result<()> {
    let sig_data = daedalus_core::format::read_at(source, footer.sig_offset, SIG_BLOCK_SIZE)?;
    let sig_size = u32::from_le_bytes([sig_data[0], sig_data[1], sig_data[2], sig_data[3]]);
    if sig_size != SIG_LEN as u32 {
        anyhow::bail!("unexpected sig_size {sig_size} (expected {SIG_LEN} for ed25519)");
    }
    let signature_bytes = &sig_data[4..SIG_BLOCK_SIZE];
    let payload = daedalus_core::format::read_at(
        source,
        footer.payload_offset,
        footer.payload_csize as usize,
    )?;
    let meta =
        daedalus_core::format::read_at(source, footer.meta_offset, footer.meta_size as usize)?;
    // The footer is hashed (and, in v6+, the stub) so downgrades and stub
    // swaps invalidate the signature.
    let hash = signature_digest(source, footer, &payload, &meta)?;
    let keys = require_trusted_keys(trusted_dir)?;
    let sig = ed25519_dalek::Signature::from_slice(signature_bytes)?;
    let verified_key = keys.iter().find_map(|(key_path, vk)| {
        // `verify_strict` rejects small-order keys/sigs (ZIP-215); same as stub.
        vk.verify_strict(&hash, &sig)
            .ok()
            .map(|()| key_path.display().to_string())
    });
    if !args.quiet {
        if let Some(path) = &verified_key {
            eprintln!("Verified against {path}");
        }
    }
    finish_verify(args, display_name, "trailing", verified_key)
}

fn verify_sisr(
    args: &VerifyArgs,
    display_name: &str,
    source: &mut Cursor<Vec<u8>>,
    footer: &Footer,
    ext: &daedalus_core::sisr_header::SisrFooterExt,
    manifest: &daedalus_core::manifest::DeltaManifest,
    trusted_dir: &Path,
) -> Result<()> {
    let payload = daedalus_core::format::read_at(
        source,
        footer.payload_offset,
        footer.payload_csize as usize,
    )?;
    let meta =
        daedalus_core::format::read_at(source, footer.meta_offset, footer.meta_size as usize)?;
    let stub = daedalus_core::format::read_at(
        source,
        0,
        usize::try_from(footer.payload_offset).context("payload offset overflow")?,
    )?;
    let meta_json: serde_json::Value =
        serde_json::from_slice(&meta).context("invalid metadata JSON")?;
    let name = meta_json.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let version = meta_json
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let ctx = daedalus_core::sisr_stage::SisrVerifyContext::new(name, version, &stub, &meta);
    let keys = require_trusted_keys(trusted_dir)?;
    let publics: Vec<VerifyingKey> = keys.iter().map(|(_, vk)| *vk).collect();
    verify_embedded_sisr_bound(ext, manifest, &payload, &publics, &ctx)
        .context("embedded SISR signature verification failed")?;
    let verified_key = keys
        .iter()
        .find(|(_, vk)| {
            let Ok(bytes) = manifest.serialize() else {
                return false;
            };
            daedalus_core::sisr_stage::verify(&bytes, &ext.merkle_root, &ext.signature, vk)
        })
        .map(|(p, _)| p.display().to_string());
    if !args.quiet {
        if let Some(path) = &verified_key {
            eprintln!("Verified SISR manifest against {path}");
        }
    }
    finish_verify(args, display_name, "sisr", verified_key)
}

fn finish_verify(
    args: &VerifyArgs,
    display_name: &str,
    scheme: &str,
    verified_key: Option<String>,
) -> Result<()> {
    let verified = verified_key.is_some();
    if args.json {
        write_json_output(
            &serde_json::json!({
                "file": display_name,
                "signed": true,
                "scheme": scheme,
                "verified": verified,
                "key": verified_key,
            }),
            args.output.as_deref(),
        )?;
    }
    // `verified:false` must never be a successful CI gate.
    if !verified {
        anyhow::bail!("signature does not match any trusted key");
    }
    if !args.quiet && !args.json {
        eprintln!("OK: signature verified ({scheme})");
    }
    Ok(())
}

/// Authenticate an artifact that arrived from a registry or URL before it is
/// written to disk or executed. Both schemes are checked against the local
/// trust store; unsigned artifacts fail closed.
pub(crate) fn verify_artifact_bytes(bytes: &[u8], trusted_dir: &Path) -> Result<String> {
    let mut source = Cursor::new(bytes.to_vec());
    let footer = Footer::read_from(&mut source).context("invalid daedalus footer")?;
    let has_sig_block = footer.format_version >= 3 && footer.sig_offset != 0;
    if has_sig_block != footer.is_signed() {
        anyhow::bail!("inconsistent signature state (flag/offset mismatch)");
    }
    let keys = require_trusted_keys(trusted_dir)?;
    let publics: Vec<VerifyingKey> = keys.iter().map(|(_, vk)| *vk).collect();

    if has_sig_block {
        let sig_data =
            daedalus_core::format::read_at(&mut source, footer.sig_offset, SIG_BLOCK_SIZE)?;
        let sig_size = u32::from_le_bytes([sig_data[0], sig_data[1], sig_data[2], sig_data[3]]);
        if sig_size != SIG_LEN as u32 {
            anyhow::bail!("unexpected sig_size {sig_size} (expected {SIG_LEN})");
        }
        let sig = ed25519_dalek::Signature::from_slice(&sig_data[4..SIG_BLOCK_SIZE])?;
        let payload = daedalus_core::format::read_at(
            &mut source,
            footer.payload_offset,
            footer.payload_csize as usize,
        )?;
        let meta = daedalus_core::format::read_at(
            &mut source,
            footer.meta_offset,
            footer.meta_size as usize,
        )?;
        let digest = signature_digest(&mut source, &footer, &payload, &meta)?;
        if !publics
            .iter()
            .any(|vk| vk.verify_strict(&digest, &sig).is_ok())
        {
            anyhow::bail!("signature does not match any trusted key");
        }
        return Ok("trailing".to_string());
    }

    if let Some((ext, manifest)) = read_sisr(&mut source).context("failed to read SISR section")? {
        if ext.signature != [0u8; 64] {
            let payload = daedalus_core::format::read_at(
                &mut source,
                footer.payload_offset,
                footer.payload_csize as usize,
            )?;
            let meta = daedalus_core::format::read_at(
                &mut source,
                footer.meta_offset,
                footer.meta_size as usize,
            )?;
            let stub_len =
                usize::try_from(footer.payload_offset).context("payload offset overflow")?;
            let stub = daedalus_core::format::read_at(&mut source, 0, stub_len)?;
            let meta_json: serde_json::Value =
                serde_json::from_slice(&meta).context("invalid metadata JSON")?;
            let name = meta_json.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let version = meta_json
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let ctx =
                daedalus_core::sisr_stage::SisrVerifyContext::new(name, version, &stub, &meta);
            verify_embedded_sisr_bound(&ext, &manifest, &payload, &publics, &ctx)
                .context("embedded SISR signature verification failed")?;
            return Ok("sisr".to_string());
        }
    }
    anyhow::bail!("artifact is unsigned")
}

/// Load all trusted Ed25519 public keys from a directory.
pub(crate) fn load_trusted_keys(dir: &PathBuf) -> Result<Vec<(PathBuf, VerifyingKey)>> {
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
