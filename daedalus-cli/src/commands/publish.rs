use anyhow::{Context, Result};
use clap::Args;
use std::path::PathBuf;

#[derive(Args)]
pub struct PublishArgs {
    /// Path to the .daedalus file to publish
    #[arg(value_name = "FILE")]
    pub file: PathBuf,

    /// Registry URL to upload to (required, cannot use placeholder)
    #[arg(long, env = "DAEDALUS_REGISTRY")]
    pub registry: Option<String>,

    /// Authentication token for the registry
    #[arg(long, env = "DAEDALUS_TOKEN")]
    pub token: Option<String>,

    /// Skip upload, just validate the file and print info
    #[arg(long)]
    pub dry_run: bool,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Disable all interactive prompts (for CI/scripts)
    #[arg(long, global = true)]
    pub no_input: bool,
}

const DEFAULT_REGISTRY_PLACEHOLDER: &str = "https://daedalus.example.com";

/// run - publish a .daedalus file to a remote registry.
/// @args: command arguments
///
/// Description:
/// Uploads a .daedalus binary to a registry endpoint via multipart POST.
/// Requires --registry or DAEDALUS_REGISTRY env var.
///
/// Return: Result containing Result<()>
pub fn run(args: PublishArgs) -> Result<()> {
    let file = args.file.canonicalize().context("failed to find file")?;

    if file.extension().is_none_or(|e| e != "daedalus") {
        anyhow::bail!(
            "{} is not a .daedalus file (expected .daedalus extension)",
            file.display()
        );
    }

    let meta_size = file
        .metadata()
        .context("failed to read file metadata")?
        .len();

    // Registry URL is mandatory - no placeholder allowed as default
    let registry = match &args.registry {
        Some(url) => url.clone(),
        None => {
            anyhow::bail!("registry URL is required (use --registry or DAEDALUS_REGISTRY env var)")
        }
    };

    // Reject placeholder URLs that would result in a non-functional upload
    if registry.contains(DEFAULT_REGISTRY_PLACEHOLDER) {
        anyhow::bail!(
            "cannot use placeholder registry URL '{}'; please provide a valid registry endpoint with --registry or DAEDALUS_REGISTRY",
            DEFAULT_REGISTRY_PLACEHOLDER
        );
    }

    if args.verbose {
        eprintln!("[daedalus] publish: {}", file.display());
        eprintln!("  size: {meta_size} bytes");
        eprintln!("  registry: {}", registry);
    }

    if args.dry_run {
        eprintln!("Would publish {} to {}", file.display(), registry);
        return Ok(());
    }

    let token = args.token.or_else(|| std::env::var("DAEDALUS_TOKEN").ok());

    eprintln!(
        "Publishing {}...",
        file.file_name().unwrap_or_default().to_string_lossy()
    );

    let content = std::fs::read(&file).context("failed to read .daedalus file")?;

    // Build the multipart upload request using reqwest (blocking)
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_mins(5))
        .build()
        .context("failed to create HTTP client")?;

    let part = reqwest::blocking::multipart::Part::bytes(content);
    let form = reqwest::blocking::multipart::Form::new().part("file", part);

    let mut request = client.post(&registry).multipart(form);

    if let Some(ref t) = token {
        request = request.header("Authorization", format!("Bearer {t}"));
    }

    let response = request.send().context("failed to upload .daedalus file")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        anyhow::bail!("upload failed (HTTP {status}): {body}");
    }

    eprintln!("  Upload complete (HTTP {})", response.status());
    Ok(())
}
