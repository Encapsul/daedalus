use anyhow::{Context, Result};
use clap::Args;
use daedalus_core::format::{Footer, ARCH_AARCH64, ARCH_X86_64};
use daedalus_core::paths::format_size;
use std::path::PathBuf;

#[derive(Args)]
pub struct InspectArgs {
    /// Path to the .daedalus file
    pub file: PathBuf,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,

    /// Write JSON output to file (requires --json)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Dry run — show what would be done without doing it
    #[arg(long)]
    pub dry_run: bool,

    /// Generate SBOM (SPDX JSON) instead of default metadata
    #[arg(short = 'S', long)]
    pub sbom: bool,
}

/// run - run.
/// @args: command arguments
///
/// Description:
///
/// Return: Result containing Result<()>
pub fn run(args: InspectArgs) -> Result<()> {
    if args.dry_run {
        eprintln!("Would inspect: {}", args.file.display());
        return Ok(());
    }

    let mut f = std::fs::File::open(&args.file)
        .with_context(|| {
            let name = args.file.display();
            if !args.file.exists() {
                anyhow::anyhow!("file not found: {name}")
            } else {
                anyhow::anyhow!("cannot open file: {name}")
            }
        })
        .with_context(|| format!("check the path and try again: {}", args.file.display()))?;
    let footer = Footer::read_from(&mut f).context("failed to read daedalus footer")?;

    let arch_name = match footer.arch {
        ARCH_X86_64 => "x86_64",
        ARCH_AARCH64 => "aarch64",
        _ => "unknown",
    };

    let payload =
        daedalus_core::format::read_at(&mut f, footer.meta_offset, footer.meta_size as usize)
            .context("failed to read metadata payload")?;
    let meta: serde_json::Value =
        serde_json::from_slice(&payload).context("failed to parse metadata JSON")?;

    if args.sbom {
        let sbom = generate_sbom(&args.file, &meta, arch_name, &footer);
        let json_str = serde_json::to_string_pretty(&sbom)?;
        if let Some(ref path) = args.output {
            std::fs::write(path, &json_str)
                .with_context(|| format!("failed to write to {}", path.display()))?;
            eprintln!("Wrote SBOM to {}", path.display());
        } else {
            println!("{json_str}");
        }
        return Ok(());
    }

    if args.json {
        let info = serde_json::json!({
            "file": args.file.display().to_string(),
            "format_version": footer.format_version,
            "arch": arch_name,
            "signed": footer.is_signed(),
            "payload_offset": footer.payload_offset,
            "payload_compressed_size": footer.payload_csize,
            "payload_uncompressed_size": footer.payload_usize,
            "payload_sha256": footer.sha256_hex(),
            "meta": meta,
        });
        let json_str = serde_json::to_string_pretty(&info)?;
        if let Some(ref path) = args.output {
            std::fs::write(path, &json_str)
                .with_context(|| format!("failed to write to {}", path.display()))?;
            eprintln!("Wrote JSON to {}", path.display());
        } else {
            println!("{json_str}");
        }
    } else {
        println!("File:        {}", args.file.display());
        println!("Format:      v{}", footer.format_version);
        println!("Arch:        {arch_name}");
        println!("Signed:      {}", footer.is_signed());
        println!(
            "Payload:     {} -> {}",
            format_size(footer.payload_csize),
            format_size(footer.payload_usize)
        );
        println!("SHA-256:     {}", &footer.sha256_hex()[..16]);

        // Signature info
        if footer.is_signed() {
            let sig_block_offset = footer.payload_offset + footer.payload_csize;
            println!("Sig offset:  0x{sig_block_offset:X}");
            println!("Sig size:    64 bytes (ed25519)");
        }

        if let Some(name) = meta.get("name").and_then(|v| v.as_str()) {
            println!("Name:        {name}");
        }
        if let Some(rt) = meta.get("runtime").and_then(|v| v.as_str()) {
            println!("Runtime:     {rt}");
        }
        if let Some(v) = meta.get("version").and_then(|v| v.as_str()) {
            println!("Version:     {v}");
        }
        if let Some(a) = meta.get("author").and_then(|v| v.as_str()) {
            println!("Author:      {a}");
        }
        if let Some(d) = meta.get("description").and_then(|v| v.as_str()) {
            println!("Description: {d}");
        }
        if let Some(l) = meta.get("license").and_then(|v| v.as_str()) {
            println!("License:     {l}");
        }

        // Isolation level
        if let Some(iso) = meta.get("isolation").and_then(|v| v.as_u64()) {
            println!("Isolation:   level {iso}");
        }

        // Entrypoint
        if let Some(ep) = meta.get("entrypoint").and_then(|v| v.as_array()) {
            let parts: Vec<&str> = ep.iter().filter_map(|v| v.as_str()).collect();
            println!("Entrypoint:  {}", parts.join(" "));
        }

        // Created timestamp
        if let Some(created) = meta.get("created").and_then(|v| v.as_str()) {
            println!("Created:     {created}");
        }

        // Env vars
        if let Some(env) = meta.get("env").and_then(|v| v.as_object()) {
            if !env.is_empty() {
                println!("Env vars:    {} var(s)", env.len());
                for (k, v) in env {
                    let val = v.as_str().unwrap_or("...");
                    println!("  {k}={val}");
                }
            }
        }

        // Layers
        if let Some(layers) = meta.get("layers").and_then(|v| v.as_array()) {
            println!("Layers:      {}", layers.len());
            for (i, layer) in layers.iter().enumerate() {
                let name = layer.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let usize = layer.get("usize").and_then(|v| v.as_u64()).unwrap_or(0);
                println!("  [{i}] {name} ({})", format_size(usize));
            }
        }

        // App/runtime hashes
        if let Some(app_hash) = meta.get("app_hash").and_then(|v| v.as_str()) {
            println!("App hash:    {}...", &app_hash[..16.min(app_hash.len())]);
        }
        if let Some(rt_hash) = meta.get("rt_deps_hash").and_then(|v| v.as_str()) {
            println!("RT hash:     {}...", &rt_hash[..16.min(rt_hash.len())]);
        }
    }

    Ok(())
}

/// generate_sbom - generate sbom.
///
/// Description:
///
/// Return: nothing
fn generate_sbom(
    _file: &std::path::Path,
    meta: &serde_json::Value,
    arch: &str,
    footer: &daedalus_core::format::Footer,
) -> serde_json::Value {
    use serde_json::json;

    let name = meta
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let version = meta
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("0.0.0");
    let runtime = meta
        .get("runtime")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let mut components = Vec::new();
    components.push(json!({
        "type": "application",
        "bom-ref": format!("app-{name}"),
        "name": name,
        "version": version,
        "properties": {
            "runtime": runtime,
            "arch": arch,
            "format_version": footer.format_version,
            "sha256": footer.sha256_hex(),
        }
    }));

    if let Some(layers) = meta.get("layers").and_then(|v| v.as_array()) {
        for layer in layers {
            let lname = layer.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            components.push(json!({
                "type": "library",
                "bom-ref": format!("layer-{lname}"),
                "name": lname,
                "version": version,
            }));
        }
    }

    json!({
        "spdxVersion": "SPDX-2.3",
        "documentNamespace": format!("https://daedalus.dev/spdx/{name}-{version}"),
        "id": "SPDXRef-DOCUMENT",
        "name": format!("daedalus SBOM for {name}"),
        "creationInfo": {
            "created": meta.get("created").and_then(|v| v.as_str())
                .unwrap_or(&chrono::Utc::now().to_rfc3339()),
            "creators": [
                {"tool": "daedalus", "version": env!("CARGO_PKG_VERSION")}
            ],
        },
        "documentDescribes": ["SPDXRef-Package-1"],
        "packages": components,
    })
}
