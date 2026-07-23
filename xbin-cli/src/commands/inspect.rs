use anyhow::{Context, Result};
use clap::Args;
use std::path::PathBuf;
use xbin_core::format::{Footer, ARCH_AARCH64, ARCH_X86_64, FLAG_SIGNED};

#[derive(Args)]
pub struct InspectArgs {
    /// Path to the .xbin file
    pub file: PathBuf,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,

    /// Dry run — show what would be done without doing it
    #[arg(long)]
    pub dry_run: bool,
}

pub fn run(args: InspectArgs) -> Result<()> {
    if args.dry_run {
        eprintln!("Would inspect: {}", args.file.display());
        return Ok(());
    }

    let mut f = std::fs::File::open(&args.file)
        .with_context(|| format!("failed to open {}", args.file.display()))?;
    let footer = Footer::read_from(&mut f).context("failed to read xbin footer")?;

    let arch_name = match footer.arch {
        ARCH_X86_64 => "x86_64",
        ARCH_AARCH64 => "aarch64",
        _ => "unknown",
    };

    let payload = xbin_core::format::read_at(&mut f, footer.meta_offset, footer.meta_size as usize)
        .context("failed to read metadata payload")?;
    let meta: serde_json::Value =
        serde_json::from_slice(&payload).context("failed to parse metadata JSON")?;

    if args.json {
        let info = serde_json::json!({
            "file": args.file.display().to_string(),
            "format_version": footer.format_version,
            "arch": arch_name,
            "signed": footer.is_signed(),
            "encrypted": footer.flags & FLAG_SIGNED == 0 && footer.format_version >= 4,
            "payload_offset": footer.payload_offset,
            "payload_compressed_size": footer.payload_csize,
            "payload_uncompressed_size": footer.payload_usize,
            "payload_sha256": footer.sha256_hex(),
            "meta": meta,
        });
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        eprintln!("File:        {}", args.file.display());
        eprintln!("Format:      v{}", footer.format_version);
        eprintln!("Arch:        {arch_name}");
        eprintln!("Signed:      {}", footer.is_signed());
        eprintln!("Encrypted:   {}", footer.format_version >= 4);
        eprintln!(
            "Payload:     {} → {}",
            format_size(footer.payload_csize),
            format_size(footer.payload_usize)
        );
        eprintln!("SHA-256:     {}", &footer.sha256_hex()[..16]);

        if let Some(name) = meta.get("name").and_then(|v| v.as_str()) {
            eprintln!("Name:        {name}");
        }
        if let Some(rt) = meta.get("runtime").and_then(|v| v.as_str()) {
            eprintln!("Runtime:     {rt}");
        }
        if let Some(v) = meta.get("version").and_then(|v| v.as_str()) {
            eprintln!("Version:     {v}");
        }
        if let Some(a) = meta.get("author").and_then(|v| v.as_str()) {
            eprintln!("Author:      {a}");
        }
        if let Some(d) = meta.get("description").and_then(|v| v.as_str()) {
            eprintln!("Description: {d}");
        }
        if let Some(l) = meta.get("license").and_then(|v| v.as_str()) {
            eprintln!("License:     {l}");
        }

        if let Some(env) = meta.get("env").and_then(|v| v.as_object()) {
            if !env.is_empty() {
                eprintln!("Env vars:    {}", env.len());
            }
        }

        if let Some(layers) = meta.get("layers").and_then(|v| v.as_array()) {
            eprintln!("Layers:      {}", layers.len());
            for (i, layer) in layers.iter().enumerate() {
                let name = layer.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let usize = layer.get("usize").and_then(|v| v.as_u64()).unwrap_or(0);
                eprintln!("  [{i}] {name} ({})", format_size(usize));
            }
        }
    }

    Ok(())
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes}B");
    }
    if bytes < 1024 * 1024 {
        return format!("{:.1}KB", bytes as f64 / 1024.0);
    }
    format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
}
