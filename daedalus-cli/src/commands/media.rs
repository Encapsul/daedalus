//! Stage and verify SISR update media for USB / air-gapped courier delivery.
//!
//! Publisher keeps the private key offline. Media carries `manifest` +
//! `chunks/<hex>` (+ optional `.pub`). Recipients trust the pub, then apply
//! via `DAEDALUS_SISR_MANIFEST` or `--daedalus-update` against a local HTTP
//! root of the same layout.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use daedalus_core::format::Footer;
use daedalus_core::paths::trusted_keys_dir;
use daedalus_core::sisr_header::read_sisr;
use daedalus_core::sisr_stage::{
    embedded_chunk_hashes, export_media, verify_media, MediaExportStats, MediaVerifyStats,
    RemoteManifest,
};
use ed25519_dalek::VerifyingKey;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

#[derive(Args)]
pub struct MediaArgs {
    #[command(subcommand)]
    pub command: MediaCommand,
}

#[derive(Subcommand)]
pub enum MediaCommand {
    /// Write `manifest` + `chunks/` for a SISR-enabled `.de` (USB / courier)
    Stage(StageArgs),
    /// Check a staged media root (signature, Merkle, chunk files) offline
    Verify(VerifyMediaArgs),
}

#[derive(Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct StageArgs {
    /// SISR-enabled `.de` / `.daedalus` to export (v2 / newest release)
    pub binary: PathBuf,

    /// Output directory (created if missing)
    #[arg(short, long)]
    pub output: PathBuf,

    /// Prior installed binary: omit chunks the recipient already has
    #[arg(long)]
    pub against: Option<PathBuf>,

    /// Copy a publisher `.pub` into the media root (never a private key)
    #[arg(long)]
    pub pubkey: Option<PathBuf>,

    /// Overwrite an existing output directory
    #[arg(short, long)]
    pub force: bool,

    /// Suppress non-error output
    #[arg(short, long)]
    pub quiet: bool,

    /// Machine-readable result
    #[arg(long)]
    pub json: bool,

    /// Disable interactive prompts
    #[arg(long, global = true)]
    pub no_input: bool,
}

#[derive(Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct VerifyMediaArgs {
    /// Media root containing `manifest` and `chunks/`
    pub media: PathBuf,

    /// Directory of trusted public keys (default: system trust store)
    #[arg(long)]
    pub trusted_dir: Option<PathBuf>,

    /// Extra publisher `.pub` to accept for this check (never a private key)
    #[arg(long)]
    pub pubkey: Option<PathBuf>,

    /// Prior installed binary: allow chunks omitted from a delta stage
    #[arg(long)]
    pub against: Option<PathBuf>,

    /// Suppress non-error output
    #[arg(short, long)]
    pub quiet: bool,

    /// Machine-readable result
    #[arg(long)]
    pub json: bool,

    /// Disable interactive prompts
    #[arg(long, global = true)]
    pub no_input: bool,
}

/// Dispatch `daedalus media …`.
pub fn run(args: MediaArgs) -> Result<()> {
    match args.command {
        MediaCommand::Stage(stage) => stage_run(stage),
        MediaCommand::Verify(verify) => verify_run(verify),
    }
}

fn stage_run(args: StageArgs) -> Result<()> {
    prepare_output(&args.output, args.force)?;
    let payload = read_payload(&args.binary)?;
    let remote = load_remote_manifest(&args.binary)?;
    let reuse = against_hashes(args.against.as_deref())?;
    let stats = export_media(&payload, &remote, &args.output, reuse.as_ref())
        .context("failed to stage SISR update media")?;
    if let Some(pub_path) = &args.pubkey {
        copy_pubkey(pub_path, &args.output)?;
    }
    report_stage(&args, &stats)
}

fn verify_run(args: VerifyMediaArgs) -> Result<()> {
    let publics = load_verify_keys(&args)?;
    let reuse = against_hashes(args.against.as_deref())?;
    let stats = verify_media(&args.media, &publics, reuse.as_ref())
        .with_context(|| format!("media verify failed for {}", args.media.display()))?;
    report_verify(&args, &stats)
}

fn against_hashes(against: Option<&Path>) -> Result<Option<std::collections::HashSet<[u8; 32]>>> {
    match against {
        Some(prev) => Ok(Some(embedded_chunk_hashes(prev).with_context(|| {
            format!(
                "failed to read SISR chunk table from --against {}",
                prev.display()
            )
        })?)),
        None => Ok(None),
    }
}

fn load_verify_keys(args: &VerifyMediaArgs) -> Result<Vec<VerifyingKey>> {
    let trusted_dir = args.trusted_dir.clone().unwrap_or_else(trusted_keys_dir);
    let mut keys = crate::commands::verify::load_trusted_keys(&trusted_dir)?;
    if let Some(pub_path) = &args.pubkey {
        keys.push((pub_path.clone(), read_pubkey(pub_path)?));
    }
    // Also accept a `.pub` sitting next to the media (courier package).
    keys.extend(media_sidecar_pubs(&args.media)?);
    Ok(keys.into_iter().map(|(_, vk)| vk).collect())
}

fn media_sidecar_pubs(media: &Path) -> Result<Vec<(PathBuf, VerifyingKey)>> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(media) else {
        return Ok(out);
    };
    for entry in entries {
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("pub"))
        {
            out.push((path.clone(), read_pubkey(&path)?));
        }
    }
    Ok(out)
}

fn read_pubkey(path: &Path) -> Result<VerifyingKey> {
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pub"))
    {
        anyhow::bail!("--pubkey must be a .pub file (private keys must never ride on media)");
    }
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.len() != 32 {
        anyhow::bail!("public key must be 32 bytes, got {}", bytes.len());
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    VerifyingKey::from_bytes(&arr).context("invalid Ed25519 public key")
}

fn prepare_output(dir: &Path, force: bool) -> Result<()> {
    if dir.exists() {
        if !force {
            anyhow::bail!(
                "output {} already exists - pass --force to overwrite",
                dir.display()
            );
        }
        fs::remove_dir_all(dir).with_context(|| format!("failed to clear {}", dir.display()))?;
    }
    fs::create_dir_all(dir).with_context(|| format!("failed to create {}", dir.display()))?;
    Ok(())
}

fn read_payload(bin: &Path) -> Result<Vec<u8>> {
    let data = fs::read(bin).with_context(|| format!("failed to read {}", bin.display()))?;
    let footer = Footer::read_from(&mut Cursor::new(&data))
        .with_context(|| format!("failed to parse footer of {}", bin.display()))?;
    let start = usize::try_from(footer.payload_offset).context("payload offset overflow")?;
    let len = usize::try_from(footer.payload_csize).context("payload size overflow")?;
    let end = start.checked_add(len).context("payload range overflow")?;
    data.get(start..end)
        .map(<[u8]>::to_vec)
        .context("payload range out of file bounds")
}

fn load_remote_manifest(bin: &Path) -> Result<RemoteManifest> {
    let mut sibling = bin.to_path_buf();
    sibling.set_extension("daedalus.manifest");
    if sibling.is_file() {
        let bytes =
            fs::read(&sibling).with_context(|| format!("failed to read {}", sibling.display()))?;
        return RemoteManifest::from_bytes(&bytes)
            .with_context(|| format!("invalid remote manifest {}", sibling.display()));
    }
    remote_from_embedded(bin)
}

fn remote_from_embedded(bin: &Path) -> Result<RemoteManifest> {
    let data = fs::read(bin).with_context(|| format!("failed to read {}", bin.display()))?;
    let (ext, manifest) = read_sisr(&mut Cursor::new(&data))
        .with_context(|| format!("failed to read SISR from {}", bin.display()))?
        .context("binary has no SISR section - rebuild with --enable-sisr")?;
    Ok(RemoteManifest {
        merkle_root: ext.merkle_root,
        signature: ext.signature,
        manifest,
    })
}

fn copy_pubkey(pub_path: &Path, out_dir: &Path) -> Result<()> {
    let name = pub_path
        .file_name()
        .context("--pubkey path has no file name")?;
    let bytes = *read_pubkey(pub_path)?.as_bytes();
    fs::write(out_dir.join(name), bytes)
        .with_context(|| format!("failed to copy pubkey into {}", out_dir.display()))?;
    Ok(())
}

fn report_stage(args: &StageArgs, stats: &MediaExportStats) -> Result<()> {
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "binary": args.binary.display().to_string(),
                "output": args.output.display().to_string(),
                "chunk_count": stats.chunk_count,
                "written_chunks": stats.written_chunks,
                "written_bytes": stats.written_bytes,
                "skipped_chunks": stats.skipped_chunks,
                "against": args.against.as_ref().map(|p| p.display().to_string()),
                "pubkey_copied": args.pubkey.is_some(),
            }))?
        );
        return Ok(());
    }
    if !args.quiet {
        eprintln!(
            "Staged {} → {} ({} chunks written, {} reused/skipped, {} bytes)",
            args.binary.display(),
            args.output.display(),
            stats.written_chunks,
            stats.skipped_chunks,
            stats.written_bytes
        );
        eprintln!(
            "Check: daedalus media verify {} --against <v1.de>; then trust .pub and apply",
            args.output.display()
        );
    }
    Ok(())
}

fn report_verify(args: &VerifyMediaArgs, stats: &MediaVerifyStats) -> Result<()> {
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "media": args.media.display().to_string(),
                "verified": true,
                "chunk_count": stats.chunk_count,
                "present_chunks": stats.present_chunks,
                "reused_chunks": stats.reused_chunks,
                "present_bytes": stats.present_bytes,
                "against": args.against.as_ref().map(|p| p.display().to_string()),
            }))?
        );
        return Ok(());
    }
    if !args.quiet {
        eprintln!(
            "OK: media verified ({} chunks on media, {} reused via --against, {} bytes)",
            stats.present_chunks, stats.reused_chunks, stats.present_bytes
        );
    }
    Ok(())
}
