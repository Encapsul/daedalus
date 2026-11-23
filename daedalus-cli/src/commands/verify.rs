use anyhow::{Context, Result};
use clap::Args;
use daedalus_core::format::{read_at, signature_digest, Footer, SIG_BLOCK_SIZE, SIG_LEN};
use daedalus_core::manifest::DeltaManifest;
use daedalus_core::paths::trusted_keys_dir;
use daedalus_core::sisr_header::{read_sisr, SisrFooterExt};
use daedalus_core::sisr_stage::{verify_embedded_sisr_bound, SisrVerifyContext};
use ed25519_dalek::VerifyingKey;
use sha2::{Digest, Sha256};
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

// CLI flags stay independent booleans so `clap` keeps a flat, scriptable
// surface; grouping them into enums would change the command line.
#[allow(clippy::struct_excessive_bools)]
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

    /// Verify the full at-rest artifact in one pass: signature scheme, stub
    /// coverage, integrity hash, metadata and SISR manifest/identity binding
    #[arg(long)]
    pub all: bool,

    /// Disable all interactive prompts (for CI/scripts)
    #[arg(long, global = true)]
    pub no_input: bool,
}

/// Verify trailing Ed25519 (v3+) or embedded SISR manifest signature.
pub fn run(args: VerifyArgs) -> Result<()> {
    if args.all {
        return run_all(&args);
    }
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

/// Facts read once from the artifact, shared by every `--all` check.
struct AtRest {
    footer: Footer,
    payload: Vec<u8>,
    meta: Vec<u8>,
    stub: Vec<u8>,
    meta_json: Option<serde_json::Value>,
    sisr: Option<(SisrFooterExt, DeltaManifest)>,
    has_sig_block: bool,
}

impl AtRest {
    fn name(&self) -> &str {
        self.meta_json
            .as_ref()
            .and_then(|v| v.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
    }

    fn version(&self) -> &str {
        self.meta_json
            .as_ref()
            .and_then(|v| v.get("version"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
    }

    /// The footer records SHA-256(payload || metadata) at build time.
    fn integrity_ok(&self) -> bool {
        let mut hasher = Sha256::new();
        hasher.update(&self.payload);
        hasher.update(&self.meta);
        hasher.finalize().as_slice() == self.footer.payload_sha256
    }
}

fn read_at_rest(file: &Path) -> Result<(AtRest, Cursor<Vec<u8>>)> {
    let mut source = open_source(file)?;
    let footer = Footer::read_from(&mut source).context("failed to read daedalus footer")?;
    let has_sig_block = footer.format_version >= 3 && footer.sig_offset != 0;
    if has_sig_block != footer.is_signed() {
        anyhow::bail!("inconsistent signature state (flag/offset mismatch)");
    }
    let payload = read_at(
        &mut source,
        footer.payload_offset,
        footer.payload_csize as usize,
    )?;
    let meta = read_at(&mut source, footer.meta_offset, footer.meta_size as usize)?;
    let stub_len = usize::try_from(footer.payload_offset).context("payload offset overflow")?;
    let stub = read_at(&mut source, 0, stub_len)?;
    let meta_json = serde_json::from_slice(&meta).ok();
    let sisr = read_sisr(&mut source).context("failed to read SISR section")?;
    Ok((
        AtRest {
            footer,
            payload,
            meta,
            stub,
            meta_json,
            sisr,
            has_sig_block,
        },
        source,
    ))
}

fn at_rest_report(display_name: &str, at: &AtRest) -> serde_json::Value {
    serde_json::json!({
        "file": display_name,
        "mode": "all",
        "format_version": at.footer.format_version,
        "signed": at.footer.is_signed(),
        "encrypted": at.footer.is_encrypted(),
        "has_sisr": at.footer.has_sisr(),
        "stub_bytes": at.footer.payload_offset,
        "stub_bound_signature": at.footer.format_version >= 6,
        "integrity_sha256": at.footer.sha256_hex(),
        "integrity_ok": at.integrity_ok(),
        "metadata": at.meta_json.clone(),
        "sisr": at.sisr.as_ref().map(|(ext, manifest)| serde_json::json!({
            "manifest_version": manifest.version,
            "chunks": manifest.chunks.len(),
            "payload_len": manifest.payload_len,
            "identity": manifest.identity.is_some(),
            "sequence": manifest.identity.as_ref().map(|i| i.sequence),
            "merkle_root": hex::encode(ext.merkle_root),
            "signed": ext.signature != [0u8; 64],
        })),
    })
}

/// `--all`: verify the signature scheme and report every at-rest binding.
fn run_all(args: &VerifyArgs) -> Result<()> {
    let display_name = display_name_of(&args.file);
    let (at, mut source) = read_at_rest(&args.file)?;
    let mut report = at_rest_report(&display_name, &at);

    if !at.integrity_ok() {
        report["verified"] = serde_json::json!(false);
        emit_all(args, &report)?;
        anyhow::bail!("integrity hash mismatch: payload or metadata were modified");
    }

    let trusted_dir = args.trusted_dir.clone().unwrap_or_else(trusted_keys_dir);
    match verify_at_rest_signature(&mut source, &at, &trusted_dir) {
        Ok((scheme, key)) => {
            let verified = key.is_some();
            report["scheme"] = serde_json::json!(scheme);
            report["verified"] = serde_json::json!(verified);
            report["key"] = serde_json::json!(key);
            emit_all(args, &report)?;
            if !verified {
                anyhow::bail!("signature does not match any trusted key");
            }
            Ok(())
        }
        Err(err) => {
            report["verified"] = serde_json::json!(false);
            report["error"] = serde_json::json!(err.to_string());
            emit_all(args, &report)?;
            Err(err)
        }
    }
}

fn verify_at_rest_signature(
    source: &mut Cursor<Vec<u8>>,
    at: &AtRest,
    trusted_dir: &Path,
) -> Result<(String, Option<String>)> {
    if at.has_sig_block {
        let key = verify_trailing_signature(source, at, trusted_dir)?;
        return Ok(("trailing".to_string(), key));
    }
    if let Some((ext, manifest)) = &at.sisr {
        if ext.signature != [0u8; 64] {
            let key = verify_sisr_signature(ext, manifest, at, trusted_dir)?;
            return Ok(("sisr".to_string(), key));
        }
    }
    anyhow::bail!("file is not signed")
}

fn verify_trailing_signature(
    source: &mut Cursor<Vec<u8>>,
    at: &AtRest,
    trusted_dir: &Path,
) -> Result<Option<String>> {
    let sig_data = read_at(source, at.footer.sig_offset, SIG_BLOCK_SIZE)?;
    let sig_size = u32::from_le_bytes([sig_data[0], sig_data[1], sig_data[2], sig_data[3]]);
    if sig_size != SIG_LEN as u32 {
        anyhow::bail!("unexpected sig_size {sig_size} (expected {SIG_LEN} for ed25519)");
    }
    let signature_bytes = &sig_data[4..SIG_BLOCK_SIZE];
    let hash = signature_digest(source, &at.footer, &at.payload, &at.meta)?;
    let keys = require_trusted_keys(trusted_dir)?;
    let sig = ed25519_dalek::Signature::from_slice(signature_bytes)?;
    Ok(keys.iter().find_map(|(key_path, vk)| {
        // `verify_strict` rejects small-order keys/sigs (ZIP-215); same as stub.
        vk.verify_strict(&hash, &sig)
            .ok()
            .map(|()| key_path.display().to_string())
    }))
}

fn verify_sisr_signature(
    ext: &SisrFooterExt,
    manifest: &DeltaManifest,
    at: &AtRest,
    trusted_dir: &Path,
) -> Result<Option<String>> {
    let keys = require_trusted_keys(trusted_dir)?;
    let publics: Vec<VerifyingKey> = keys.iter().map(|(_, vk)| *vk).collect();
    let ctx = SisrVerifyContext::new(at.name(), at.version(), &at.stub, &at.meta);
    verify_embedded_sisr_bound(ext, manifest, &at.payload, &publics, &ctx)
        .context("embedded SISR signature verification failed")?;
    Ok(keys
        .iter()
        .find(|(_, vk)| {
            manifest.serialize().is_ok_and(|bytes| {
                daedalus_core::sisr_stage::verify(&bytes, &ext.merkle_root, &ext.signature, vk)
            })
        })
        .map(|(p, _)| p.display().to_string()))
}

fn emit_all(args: &VerifyArgs, report: &serde_json::Value) -> Result<()> {
    if args.json {
        return write_json_output(report, args.output.as_deref());
    }
    if !args.quiet {
        print_at_rest(report);
    }
    Ok(())
}

fn print_at_rest(report: &serde_json::Value) {
    let s = |k: &str| report.get(k).and_then(|v| v.as_str()).unwrap_or("?");
    let n = |k: &str| report.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
    let b = |k: &str| report.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    eprintln!("OK: at-rest verification — {}", s("file"));
    eprintln!("  scheme:    {}", s("scheme"));
    if let Some(key) = report.get("key").and_then(|v| v.as_str()) {
        eprintln!("  key:       {key}");
    }
    eprintln!("  format:    v{}", n("format_version"));
    eprintln!(
        "  stub:      {} byte(s){}",
        n("stub_bytes"),
        if b("stub_bound_signature") {
            " (bound by the signature)"
        } else {
            " (not covered: pre-v6 format)"
        }
    );
    eprintln!(
        "  integrity: sha256:{} ({})",
        s("integrity_sha256"),
        if b("integrity_ok") { "ok" } else { "MISMATCH" }
    );
    if let Some(meta) = report.get("metadata").filter(|v| v.is_object()) {
        let name = meta.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let version = meta.get("version").and_then(|v| v.as_str()).unwrap_or("?");
        eprintln!("  metadata:  {name} {version}");
    } else {
        eprintln!("  metadata:  unreadable");
    }
    if let Some(sisr) = report.get("sisr").filter(|v| v.is_object()) {
        eprintln!(
            "  sisr:      manifest v{}, {} chunk(s), sequence {}, {}{}",
            sisr.get("manifest_version")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            sisr.get("chunks").and_then(|v| v.as_u64()).unwrap_or(0),
            sisr.get("sequence").and_then(|v| v.as_u64()).unwrap_or(0),
            if sisr.get("identity") == Some(&serde_json::Value::Bool(true)) {
                "bound identity"
            } else {
                "legacy identity"
            },
            if sisr.get("signed") == Some(&serde_json::Value::Bool(true)) {
                ", signed"
            } else {
                ", unsigned"
            }
        );
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
