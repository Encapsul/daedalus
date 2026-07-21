use anyhow::{Context, Result};
use clap::Args;
use std::path::PathBuf;

#[derive(Args)]
pub struct CleanArgs {
    /// Remove all cached data
    #[arg(long)]
    pub all: bool,

    /// Skip confirmation
    #[arg(short, long)]
    pub force: bool,
}

pub fn run(args: CleanArgs) -> Result<()> {
    let cache_dir = cache_dir();

    if !cache_dir.exists() {
        eprintln!("Nothing to clean");
        return Ok(());
    }

    let size = dir_size(&cache_dir)?;

    if !args.force {
        eprintln!("This will remove {} ({})", cache_dir.display(), format_size(size));
        eprint!("Continue? [y/N] ");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            eprintln!("Aborted");
            return Ok(());
        }
    }

    std::fs::remove_dir_all(&cache_dir)
        .with_context(|| format!("failed to remove cache directory {}", cache_dir.display()))?;
    eprintln!("Cleaned {} ({})", cache_dir.display(), format_size(size));

    Ok(())
}

fn cache_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        PathBuf::from(xdg).join("xbin")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".cache").join("xbin")
    } else {
        PathBuf::from(".xbin").join("cache")
    }
}

fn dir_size(path: &std::path::Path) -> Result<u64> {
    let mut total = 0;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_dir() {
            total += dir_size(&entry.path())?;
        } else {
            total += meta.len();
        }
    }
    Ok(total)
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
