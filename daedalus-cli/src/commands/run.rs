use anyhow::{Context, Result};
use clap::Args;
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

/// Maximum bytes accepted from a registry before an artifact is rejected.
///
/// Downloads stream to disk in bounded chunks instead of slurping the whole
/// body into RAM, so an untrusted registry cannot OOM the client; this cap
/// additionally bounds disk usage for a single fetch.
const MAX_DOWNLOAD_BYTES: u64 = 32 << 30;

#[derive(Args)]
pub struct RunArgs {
    /// Path to the .de file, or an http(s)/registry URL from which to fetch it
    pub file: PathBuf,

    /// Arguments forwarded to the embedded app
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub app_args: Vec<String>,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Output result as JSON
    #[arg(long)]
    pub json: bool,
}

/// run - run a local .de file or download + run one from a registry URL.
/// @args: command arguments
///
/// Description:
/// URL sources (`http(s)://` and the `registry://` alias) are fetched into
/// `~/.cache/daedalus/downloads` (keyed by URL hash), validated as a `.de`
/// footer, and then executed like a local file. Re-runs reuse the cache.
///
/// Return: Result containing Result<()>
pub fn run(args: RunArgs) -> Result<()> {
    let path_arg = args.file.to_string_lossy();
    let file = if is_remote_url(&path_arg) {
        fetch_from_url(&path_arg, args.verbose)?
    } else {
        args.file.clone()
    };

    let file = file
        .canonicalize()
        .with_context(|| format!("cannot find {}", file.display()))?;

    if !file.is_file() {
        anyhow::bail!("{} is not a file", file.display());
    }

    // Verify it's a valid daedalus file before executing
    if !verify_de(&file) {
        anyhow::bail!("{} is not a valid .de file", file.display());
    }

    if args.verbose {
        eprintln!("Executing {}...", file.display());
    }

    if args.json {
        let info = serde_json::json!({
            "command": "run",
            "file": file.display().to_string(),
        });
        println!("{}", serde_json::to_string_pretty(&info)?);
    }

    let err = Command::new(&file)
        .args(&args.app_args)
        .status()
        .with_context(|| format!("failed to execute {}", file.display()))?;

    if !err.success() {
        std::process::exit(err.code().unwrap_or(1));
    }

    Ok(())
}

/// is_remote_url - whether the argument names a remote source rather than a file.
/// @s: the file/URL argument
///
/// Description:
/// Recognizes `http://`, `https://`, and the `registry://` alias.
///
/// Return: true if the argument is a URL
fn is_remote_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("registry://")
}

/// fetch_from_url - download a .de from a URL into the run cache and return its path.
/// @raw_url: the URL argument
/// @verbose: verbose output
///
/// Description:
/// Cache key is the SHA-256 of the URL, so re-running the same source reuses
/// the downloaded bytes. The cached file is re-validated as a `.de` on every
/// run and re-downloaded if corrupt.
///
/// Return: the cached .de path
fn fetch_from_url(raw_url: &str, verbose: bool) -> Result<PathBuf> {
    let http_url = if let Some(rest) = raw_url.strip_prefix("registry://") {
        format!("http://{rest}")
    } else {
        raw_url.to_string()
    };

    let key = daedalus_core::registry::format_hex(&daedalus_core::registry::content_hash(
        http_url.as_bytes(),
    ));
    let cache_dir = daedalus_core::paths::cache_dir().join("downloads");
    let cache_path = cache_dir.join(format!("{key}.de"));

    if cache_path.exists() && verify_de(&cache_path) {
        if verbose {
            eprintln!("[daedalus] using cached {}", cache_path.display());
        }
        return Ok(cache_path);
    }

    std::fs::create_dir_all(&cache_dir)
        .with_context(|| format!("failed to create {}", cache_dir.display()))?;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_mins(5))
        .build()
        .context("failed to create HTTP client")?;

    if verbose {
        eprintln!("[daedalus] downloading {http_url}");
    }

    let mut response = client.get(&http_url).send().context("download failed")?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("download failed (HTTP {status})");
    }

    // A process/attempt-unique temp name keeps two concurrent `daedalus run`s
    // of the same URL from racing each other's partial file.
    let unique = format!(
        "{key}.{}.part",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let tmp = cache_dir.join(unique);
    let mut file = std::fs::File::create(&tmp).context("failed to create temp download")?;

    // Stream the body to disk in chunks, rejecting anything over the cap
    // instead of buffering an unbounded payload.
    let mut total: u64 = 0;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = response
            .read(&mut buf)
            .context("failed to read response body")?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_DOWNLOAD_BYTES {
            let _ = std::fs::remove_file(&tmp);
            anyhow::bail!("download exceeds {MAX_DOWNLOAD_BYTES} bytes");
        }
        file.write_all(&buf[..n])?;
    }
    file.flush()?;

    if !verify_de(&tmp) {
        let _ = std::fs::remove_file(&tmp);
        anyhow::bail!("downloaded bytes from {http_url} are not a valid .de");
    }

    // Executing the cached file needs the exec bit; downloads default to 0644.
    #[cfg(unix)]
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
        .context("failed to mark downloaded file executable")?;

    std::fs::rename(&tmp, &cache_path).context("failed to finalize cached download")?;
    Ok(cache_path)
}

/// verify_de - check that a file carries a valid `.de` footer.
/// @path: the file to check
///
/// Description:
/// Reads and validates the footer magic without executing anything.
///
/// Return: true if the footer parses
fn verify_de(path: &std::path::Path) -> bool {
    use daedalus_core::format::Footer;
    match std::fs::File::open(path) {
        Ok(mut f) => Footer::read_from(&mut f).is_ok(),
        Err(_) => false,
    }
}
