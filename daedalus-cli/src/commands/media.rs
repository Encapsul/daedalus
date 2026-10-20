//! Stage SISR update media for USB / air-gapped courier delivery.
//!
//! Publisher keeps the private key offline. Media carries `manifest` +
//! `chunks/<hex>` (+ optional `.pub`). Recipients trust the pub, then apply
//! via `DAEDALUS_SISR_MANIFEST` or `--daedalus-update` against a local HTTP
//! root of the same layout.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use daedalus_core::format::Footer;
use daedalus_core::sisr_header::read_sisr;
use daedalus_core::sisr_stage::{
    embedded_chunk_hashes, export_media, MediaExportStats, RemoteManifest,
};
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

/// Dispatch `daedalus media …`.
pub fn run(args: MediaArgs) -> Result<()> {
    match args.command {
        MediaCommand::Stage(stage) => stage_run(stage),
    }
}

fn stage_run(args: StageArgs) -> Result<()> {
    prepare_output(&args.output, args.force)?;
    let payload = read_payload(&args.binary)?;
    let remote = load_remote_manifest(&args.binary)?;
    let reuse = match &args.against {
        Some(prev) => Some(embedded_chunk_hashes(prev).with_context(|| {
            format!(
                "failed to read SISR chunk table from --against {}",
                prev.display()
            )
        })?),
        None => None,
    };
    let stats = export_media(&payload, &remote, &args.output, reuse.as_ref())
        .context("failed to stage SISR update media")?;
    if let Some(pub_path) = &args.pubkey {
        copy_pubkey(pub_path, &args.output)?;
    }
    report_stage(&args, &stats)
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
    if !pub_path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pub"))
    {
        anyhow::bail!("--pubkey must be a .pub file (private keys must never ride on media)");
    }
    let bytes =
        fs::read(pub_path).with_context(|| format!("failed to read {}", pub_path.display()))?;
    if bytes.len() != 32 {
        anyhow::bail!("--pubkey must be 32 raw Ed25519 bytes, got {}", bytes.len());
    }
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
            "Recipient: trust the .pub, then DAEDALUS_SISR_MANIFEST={}/manifest ./app.de",
            args.output.display()
        );
    }
    Ok(())
}
