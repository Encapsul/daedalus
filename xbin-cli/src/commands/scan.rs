use anyhow::Result;
use clap::Args;
use std::path::{Path, PathBuf};
use xbin_core::format::{Footer, ARCH_X86_64, ARCH_AARCH64};

#[derive(Args)]
pub struct ScanArgs {
    /// Directories to scan
    #[arg(default_value = ".")]
    pub paths: Vec<PathBuf>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,

    /// Dry run — show what would be done without doing it
    #[arg(long)]
    pub dry_run: bool,
}

pub fn run(args: ScanArgs) -> Result<()> {
    if args.dry_run {
        for path in &args.paths {
            eprintln!("Would scan: {}", path.display());
        }
        return Ok(());
    }

    let mut files = Vec::new();

    for path in &args.paths {
        if path.is_dir() {
            find_xbin_files(path, &mut files)?;
        } else if path.is_file() && is_xbin_file(path) {
            files.push(path.clone());
        }
    }

    if files.is_empty() {
        anyhow::bail!("No .xbin files found");
    }

    if args.json {
        let entries: Vec<_> = files.iter().filter_map(|f| inspect_file(f)).collect();
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        eprintln!("{:<40} {:<10} {:<10} {:<8} {:<6}", "FILE", "RUNTIME", "ARCH", "SIZE", "SIGNED");
        eprintln!("{}", "-".repeat(80));
        for file in &files {
            if let Some(info) = inspect_file(file) {
                let name: String = file.file_name().map_or_else(|| "?".into(), |n| n.to_string_lossy().into());
                let runtime = info.get("runtime").and_then(|v| v.as_str()).unwrap_or("?");
                let arch = info.get("arch").and_then(|v| v.as_str()).unwrap_or("?");
                let size = info.get("payload_compressed_size").and_then(|v| v.as_u64()).unwrap_or(0);
                let signed = info.get("signed").and_then(|v| v.as_bool()).unwrap_or(false);
                eprintln!(
                    "{:<40} {:<10} {:<10} {:<8} {:<6}",
                    if name.len() > 40 { &name[name.len()-37..] } else { &name },
                    runtime,
                    arch,
                    format_size(size),
                    if signed { "yes" } else { "no" }
                );
            }
        }
        eprintln!("\n{} file(s) found", files.len());
    }

    Ok(())
}

fn find_xbin_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name == ".git" || name == "node_modules" || name == "__pycache__" || name == ".venv" {
                continue;
            }
            find_xbin_files(&path, files)?;
        } else if is_xbin_file(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn is_xbin_file(path: &Path) -> bool {
    path.extension().map_or(false, |ext| ext == "xbin")
        && std::fs::metadata(path).map_or(false, |m| m.len() > 84)
}

fn inspect_file(path: &Path) -> Option<serde_json::Value> {
    let mut f = std::fs::File::open(path).ok()?;
    let footer = Footer::read_from(&mut f).ok()?;

    let arch_name = match footer.arch {
        ARCH_X86_64 => "x86_64",
        ARCH_AARCH64 => "aarch64",
        _ => "unknown",
    };

    let meta_bytes = xbin_core::format::read_at(&mut f, footer.meta_offset, footer.meta_size as usize).ok()?;
    let meta: serde_json::Value = serde_json::from_slice(&meta_bytes).ok()?;

    Some(serde_json::json!({
        "file": path.display().to_string(),
        "format_version": footer.format_version,
        "arch": arch_name,
        "runtime": meta.get("runtime").and_then(|v| v.as_str()).unwrap_or("unknown"),
        "name": meta.get("name").and_then(|v| v.as_str()).unwrap_or("unknown"),
        "signed": footer.is_signed(),
        "payload_compressed_size": footer.payload_csize,
        "payload_uncompressed_size": footer.payload_usize,
    }))
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
