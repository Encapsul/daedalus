use anyhow::{Context, Result};
use clap::Args;
use daedalus_core::legacy::upgrade_binary;
use daedalus_core::sisr_stage::SisrBuildConfig;
use ed25519_dalek::SigningKey;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

#[derive(Args)]
pub struct UpgradeBinaryArgs {
    /// Input legacy .daedalus (built without --enable-sisr)
    pub input: PathBuf,

    /// Output path for the SISR-enabled binary
    pub output: PathBuf,

    /// SISR chunk target size in bytes
    #[arg(long, default_value_t = 64 << 10)]
    pub chunk_size: usize,

    /// Sign the SISR manifest with a 32-byte Ed25519 key
    #[arg(short, long)]
    pub key: Option<PathBuf>,

    /// Overwrite the output without confirmation
    #[arg(short, long)]
    pub force: bool,

    /// Suppress non-error output
    #[arg(short, long)]
    pub quiet: bool,

    /// Output result as JSON
    #[arg(long)]
    pub json: bool,

    /// Disable all interactive prompts (for CI/scripts)
    #[arg(long, global = true)]
    pub no_input: bool,
}

/// run - upgrade a legacy .daedalus binary to SISR-enabled format.
/// @args: command arguments
///
/// Description:
/// Repackages a v1 .daedalus binary with SISR chunking and optionally signs
/// the manifest with an Ed25519 key.
///
/// Return: Result containing Result<()>
pub fn run(args: UpgradeBinaryArgs) -> Result<()> {
    if args.output.exists() && !args.force {
        anyhow::bail!(
            "output {} already exists — pass --force to overwrite",
            args.output.display()
        );
    }
    if args.chunk_size == 0 {
        anyhow::bail!("--chunk-size must be greater than zero");
    }

    let signing_key = match &args.key {
        Some(path) => {
            warn_if_insecure_key_permissions(path);
            let key_bytes =
                Zeroizing::new(std::fs::read(path).with_context(|| {
                    format!("failed to read signing key at {}", path.display())
                })?);
            if key_bytes.len() != 32 {
                anyhow::bail!("key must be 32 bytes, got {}", key_bytes.len());
            }
            let mut key_arr = Zeroizing::new([0u8; 32]);
            key_arr.copy_from_slice(&key_bytes);
            Some(SigningKey::from_bytes(&key_arr))
        }
        None => None,
    };

    let config = SisrBuildConfig {
        enabled: true,
        chunk_target_size: args.chunk_size,
        signing_key,
    };

    let report = upgrade_binary(&args.input, &args.output, &config).with_context(|| {
        format!(
            "failed to upgrade {} (rebuild with `daedalus build --enable-sisr` if it is signed)",
            args.input.display()
        )
    })?;

    let mut manifest = args.output.clone();
    manifest.set_extension("daedalus.manifest");

    if args.json {
        let result = serde_json::json!({
            "input": args.input.display().to_string(),
            "output": args.output.display().to_string(),
            "manifest": manifest.display().to_string(),
            "input_size_bytes": report.input_size,
            "output_size_bytes": report.output_size,
            "chunk_count": report.chunk_count,
            "manifest_offset": report.manifest_offset,
            "signed": report.signed,
        });
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else if !args.quiet {
        eprintln!(
            "Upgraded {} → {} ({} bytes, {} chunks)",
            args.input.display(),
            args.output.display(),
            report.output_size,
            report.chunk_count
        );
        eprintln!("SISR manifest written: {}", manifest.display());
    }

    Ok(())
}

/// Warns when a private key file is group/other-readable (not 0600) on Unix.
fn warn_if_insecure_key_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mode = meta.permissions().mode() & 0o777;
            if mode != 0o600 {
                eprintln!(
                    "[daedalus] warning: private key {} has mode {mode:o}, expected 0600",
                    path.display()
                );
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}
