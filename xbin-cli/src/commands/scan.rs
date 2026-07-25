use anyhow::{Context, Result};
use clap::Args;
use std::path::{Path, PathBuf};
use xbin_core::format::{Footer, ARCH_AARCH64, ARCH_X86_64};
use xbin_core::paths::{cache_dir, format_size};

#[derive(Args)]
pub struct ScanArgs {
    /// Directories to scan
    #[arg(default_value = ".")]
    pub paths: Vec<PathBuf>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,

    /// Write JSON output to file (requires --json)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Dry run — show what would be done without doing it
    #[arg(long)]
    pub dry_run: bool,

    /// Show cache statistics
    #[arg(long)]
    pub cache: bool,
}

pub fn run(args: ScanArgs) -> Result<()> {
    // Show cache stats if requested
    if args.cache {
        let cache_dir = cache_dir();
        if cache_dir.exists() {
            let (count, total_size) = cache_stats(&cache_dir)?;
            if args.json {
                let json_str = serde_json::to_string_pretty(&serde_json::json!({
                    "cache_dir": cache_dir.display().to_string(),
                    "entries": count,
                    "total_size_bytes": total_size,
                    "total_size": format_size(total_size),
                }))?;
                write_json_output(&json_str, args.output.as_deref())?;
            } else {
                eprintln!("Cache:  {}", cache_dir.display());
                eprintln!("  Entries: {count}");
                eprintln!("  Size:   {}", format_size(total_size));
            }
        } else if args.json {
            write_json_output(
                &format!(
                    "{{\"cache_dir\":\"{}\",\"entries\":0,\"total_size_bytes\":0}}",
                    cache_dir.display()
                ),
                args.output.as_deref(),
            )?;
        } else {
            eprintln!("No cache found at {}", cache_dir.display());
        }
        return Ok(());
    }

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
        let json_str = serde_json::to_string_pretty(&entries)?;
        write_json_output(&json_str, args.output.as_deref())?;
    } else {
        println!(
            "{:<40} {:<10} {:<10} {:<10} {:<10} {:<8} {:<6}",
            "FILE", "NAME", "RUNTIME", "ARCH", "CREATED", "SIZE", "SIGNED"
        );
        println!("{}", "-".repeat(100));
        for file in &files {
            if let Some(info) = inspect_file(file) {
                let name: String = file
                    .file_name()
                    .map_or_else(|| "?".into(), |n| n.to_string_lossy().into());
                let app_name = info.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let runtime = info.get("runtime").and_then(|v| v.as_str()).unwrap_or("?");
                let arch = info.get("arch").and_then(|v| v.as_str()).unwrap_or("?");
                let created = info.get("created").and_then(|v| v.as_str()).unwrap_or("?");
                let size = info
                    .get("payload_compressed_size")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let signed = info
                    .get("signed")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                println!(
                    "{:<40} {:<10} {:<10} {:<10} {:<10} {:<8} {:<6}",
                    if name.len() > 40 {
                        &name[name.len() - 37..]
                    } else {
                        &name
                    },
                    if app_name.len() > 10 {
                        &app_name[..9]
                    } else {
                        app_name
                    },
                    runtime,
                    arch,
                    created,
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
            if name == ".git" || name == "node_modules" || name == "__pycache__" || name == ".venv"
            {
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

    let meta_bytes =
        xbin_core::format::read_at(&mut f, footer.meta_offset, footer.meta_size as usize).ok()?;
    let meta: serde_json::Value = serde_json::from_slice(&meta_bytes).ok()?;

    // Get file creation time
    let created = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| {
            let elapsed = t.duration_since(std::time::UNIX_EPOCH).ok()?;
            let secs = elapsed.as_secs();
            // Format as YYYY-MM-DD
            let days = secs / 86400;
            let mut year = 1970u32;
            let mut remaining = days;
            loop {
                let days_in_year = if is_leap(year) { 366 } else { 365 };
                if remaining < days_in_year {
                    break;
                }
                remaining -= days_in_year;
                year += 1;
            }
            let month_days = [
                31,
                if is_leap(year) { 29 } else { 28 },
                31,
                30,
                31,
                30,
                31,
                31,
                30,
                31,
                30,
                31,
            ];
            let mut month = 1u32;
            for &md in &month_days {
                if remaining < md {
                    break;
                }
                remaining -= md;
                month += 1;
            }
            Some(format!("{year:04}-{month:02}-{:02}", remaining + 1))
        })
        .unwrap_or_else(|| "?".into());

    Some(serde_json::json!({
        "file": path.display().to_string(),
        "format_version": footer.format_version,
        "arch": arch_name,
        "runtime": meta.get("runtime").and_then(|v| v.as_str()).unwrap_or("unknown"),
        "name": meta.get("name").and_then(|v| v.as_str()).unwrap_or("unknown"),
        "version": meta.get("version").and_then(|v| v.as_str()).unwrap_or(""),
        "author": meta.get("author").and_then(|v| v.as_str()).unwrap_or(""),
        "isolation": meta.get("isolation").and_then(|v| v.as_u64()).unwrap_or(1),
        "created": created,
        "signed": footer.is_signed(),
        "payload_compressed_size": footer.payload_csize,
        "payload_uncompressed_size": footer.payload_usize,
    }))
}

fn is_leap(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

fn cache_stats(dir: &Path) -> Result<(usize, u64)> {
    let mut count = 0usize;
    let mut total = 0u64;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_dir() {
            count += 1;
            let (sub_count, sub_size) = cache_stats(&entry.path())?;
            count += sub_count;
            total += sub_size;
        } else {
            count += 1;
            total += meta.len();
        }
    }
    Ok((count, total))
}

fn write_json_output(json_str: &str, output: Option<&Path>) -> Result<()> {
    if let Some(path) = output {
        std::fs::write(path, json_str)
            .with_context(|| format!("failed to write to {}", path.display()))?;
        eprintln!("Wrote JSON to {}", path.display());
    } else {
        println!("{json_str}");
    }
    Ok(())
}
