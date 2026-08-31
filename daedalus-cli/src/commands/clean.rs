use anyhow::{Context, Result};
use clap::Args;
use daedalus_core::paths::{cache_dir, format_size, BuildCache};

#[derive(Args)]
pub struct CleanArgs {
    /// Remove all cached data
    #[arg(long)]
    pub all: bool,

    /// Garbage-collect expired cache entries (TTL-based)
    #[arg(long)]
    pub gc: bool,

    /// Skip confirmation
    #[arg(short, long)]
    pub force: bool,

    /// Disable all interactive prompts (for CI/scripts)
    #[arg(long, global = true)]
    pub no_input: bool,
}

/// run - clean the daedalus build cache.
/// @args: command arguments
///
/// Description:
/// Removes the daedalus cache directory or garbage-collects expired entries.
/// Prompts for confirmation unless --force or --no-input is passed.
///
/// Return: Result containing Result<()>
pub fn run(args: CleanArgs) -> Result<()> {
    if args.gc {
        let cache = BuildCache::new(std::path::Path::new("."), 50);
        cache.gc();
        eprintln!("Cache garbage-collected (expired entries removed)");
        return Ok(());
    }

    let cache_dir = cache_dir();

    if !cache_dir.exists() {
        eprintln!("Nothing to clean");
        return Ok(());
    }

    let size = dir_size(&cache_dir)?;

    if !args.force && !args.no_input {
        if !is_interactive() {
            anyhow::bail!(
                "interactive prompt required; pass --force or --no-input for non-interactive use"
            );
        }
        eprintln!(
            "This will remove {} ({})",
            cache_dir.display(),
            format_size(size)
        );
        eprint!("Continue? [y/N] ");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            eprintln!("Aborted");
            return Ok(());
        }
    } else if args.no_input && !args.force {
        anyhow::bail!(
            "--no-input passed but operation requires confirmation; pass --force to skip"
        );
    }

    std::fs::remove_dir_all(&cache_dir)
        .with_context(|| format!("failed to remove cache directory {}", cache_dir.display()))?;
    eprintln!("Cleaned {} ({})", cache_dir.display(), format_size(size));

    Ok(())
}

/// is_interactive - check whether interactive.
///
/// Description:
///
/// Return: true or false
fn is_interactive() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

/// dir_size - compute total size of a directory tree.
/// @path: file or directory path
///
/// Description:
/// Recursively sums file sizes under path, including subdirectory contents.
///
/// Return: Result containing Result<u64>
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
