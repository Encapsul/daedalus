use anyhow::{Context, Result};
use clap::Args;
use std::path::PathBuf;

const DEFAULT_REGISTRY: &str = "https://xbin.example.com/api/v1/upload";

#[derive(Args)]
pub struct PublishArgs {
    /// Path to the .xbin file to publish
    #[arg(value_name = "FILE")]
    pub file: PathBuf,

    /// Registry URL to upload to
    #[arg(long, default_value = DEFAULT_REGISTRY)]
    pub registry: String,

    /// Authentication token for the registry
    #[arg(long, env = "XBIN_TOKEN")]
    pub token: Option<String>,

    /// Skip upload, just validate the file and print info
    #[arg(long)]
    pub dry_run: bool,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,
}

pub fn run(args: PublishArgs) -> Result<()> {
    let file = args.file.canonicalize().context("failed to find file")?;

    if file.extension().is_none_or(|e| e != "xbin") {
        anyhow::bail!(
            "{} is not a .xbin file (expected .xbin extension)",
            file.display()
        );
    }

    let meta_size = file
        .metadata()
        .context("failed to read file metadata")?
        .len();

    if args.verbose {
        eprintln!("[xbin] publish: {}", file.display());
        eprintln!("  size: {meta_size} bytes");
        eprintln!("  registry: {}", args.registry);
    }

    if args.dry_run {
        eprintln!("Would publish {} to {}", file.display(), args.registry);
        return Ok(());
    }

    let token = args.token.or_else(|| std::env::var("XBIN_TOKEN").ok());

    eprintln!(
        "Publishing {}...",
        file.file_name().unwrap_or_default().to_string_lossy()
    );

    let content = std::fs::read(&file).context("failed to read .xbin file")?;

    // Build the multipart upload request using reqwest (blocking)
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_mins(5))
        .build()
        .context("failed to create HTTP client")?;

    let part = reqwest::blocking::multipart::Part::bytes(content);
    let form = reqwest::blocking::multipart::Form::new().part("file", part);

    let mut request = client.post(&args.registry).multipart(form);

    if let Some(ref t) = token {
        request = request.header("Authorization", format!("Bearer {t}"));
    }

    let response = request.send().context("failed to upload .xbin file")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        anyhow::bail!("upload failed (HTTP {status}): {body}");
    }

    eprintln!("  Upload complete (HTTP {})", response.status());
    Ok(())
}
