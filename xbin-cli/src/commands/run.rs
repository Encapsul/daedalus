use anyhow::{Context, Result};
use clap::Args;
use std::path::PathBuf;
use std::process::Command;

#[derive(Args)]
pub struct RunArgs {
    /// Path to the .xbin file to execute
    pub file: PathBuf,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,
}

pub fn run(args: RunArgs) -> Result<()> {
    let file = args
        .file
        .canonicalize()
        .with_context(|| format!("cannot find {}", args.file.display()))?;

    if !file.is_file() {
        anyhow::bail!("{} is not a file", file.display());
    }

    // Verify it's a valid xbin file before executing
    {
        use xbin_core::format::Footer;
        let mut f = std::fs::File::open(&file)
            .with_context(|| format!("failed to open {}", file.display()))?;
        Footer::read_from(&mut f).context("invalid .xbin file — bad footer")?;
    }

    if args.verbose {
        eprintln!("Executing {}...", file.display());
    }

    // Execute the binary directly — the stub handles extraction at runtime
    let err = Command::new(&file)
        .args(std::env::args_os().skip(3)) // skip: xbin run <file>
        .status()
        .with_context(|| format!("failed to execute {}", file.display()))?;

    if !err.success() {
        std::process::exit(err.code().unwrap_or(1));
    }

    Ok(())
}
