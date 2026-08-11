use anyhow::{Context, Result};
use clap::Args;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use erebus_core::format::Footer;

#[derive(Args)]
pub struct SelftestArgs {
    /// Path to the .erebus file to test
    pub file: PathBuf,

    /// Test mode: `auto`, `server`, or `cli`
    #[arg(long, default_value = "auto")]
    pub mode: String,

    /// Liveness timeout in seconds (after 2s crash window)
    #[arg(short, long, default_value_t = 3)]
    pub timeout: u64,

    /// HTTP liveness probe URL (e.g. `http://localhost:8080/health`)
    #[arg(long)]
    pub probe: Option<String>,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,
}

pub fn run(args: SelftestArgs) -> Result<()> {
    let path = args
        .file
        .canonicalize()
        .with_context(|| format!("cannot find {}", args.file.display()))?;

    if !path.is_file() {
        anyhow::bail!("{} is not a file", path.display());
    }

    // Read footer + metadata
    let mut f =
        std::fs::File::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
    let footer = Footer::read_from(&mut f).context("invalid .erebus file")?;
    let meta_bytes =
        erebus_core::format::read_at(&mut f, footer.meta_offset, footer.meta_size as usize)
            .context("failed to read metadata")?;
    let meta: serde_json::Value =
        serde_json::from_slice(&meta_bytes).context("failed to parse metadata JSON")?;

    // Determine mode
    let mode = if args.mode == "auto" {
        detect_mode(&meta)
    } else {
        args.mode.clone()
    };

    let effective_timeout = Duration::from_secs(2 + args.timeout);

    if args.verbose {
        let rt = meta.get("runtime").and_then(|v| v.as_str()).unwrap_or("?");
        let ep: Vec<&str> = meta
            .get("entrypoint")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        eprintln!(
            "[erebus] selftest: {}  runtime={rt}  mode={mode}  timeout={}",
            path.file_name()
                .map_or_else(|| "?".to_string(), |n| n.to_string_lossy().into()),
            effective_timeout.as_secs(),
        );
        if !ep.is_empty() {
            eprintln!("[erebus] selftest: entrypoint={}", ep.join(" "));
        }
    }

    // Ephemeral cache
    let tmp = tempfile::tempdir().context("failed to create temp dir")?;
    let cache_dir = tmp.path().join("cache");

    // Launch the binary
    let mut cmd = Command::new(&path);
    cmd.env("XDG_CACHE_HOME", &cache_dir);

    if mode == "cli" {
        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
    } else {
        cmd.stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
    }

    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to launch {}", path.display()))?;

    if args.verbose {
        eprintln!("[erebus] selftest: started pid={}", child.id());
    }

    let rc = wait_and_observe(
        &mut child,
        args.timeout,
        &mode,
        args.probe.as_deref(),
        args.verbose,
    )?;

    if args.verbose {
        let label = match rc {
            0 => "PASS",
            1 => "FAIL",
            2 => "DEGRADED",
            _ => "UNKNOWN",
        };
        eprintln!("[erebus] selftest: {label}");
    }

    std::process::exit(rc);
}

fn detect_mode(meta: &serde_json::Value) -> String {
    if meta.get("services").is_some() {
        return "server".into();
    }

    let runtime = meta.get("runtime").and_then(|v| v.as_str()).unwrap_or("");
    if runtime == "python" || runtime == "node" {
        return "server".into();
    }

    "cli".into()
}

fn wait_and_observe(
    child: &mut std::process::Child,
    timeout_secs: u64,
    mode: &str,
    probe: Option<&str>,
    verbose: bool,
) -> Result<i32> {
    let crash_deadline = Instant::now() + Duration::from_secs(2);
    let end = Instant::now() + Duration::from_secs(2 + timeout_secs);

    // Phase 1: crash check (0-2s)
    loop {
        if Instant::now() >= crash_deadline {
            break;
        }
        if let Some(status) = child.try_wait()? {
            let code = status.code().unwrap_or(1);
            if code != 0 {
                report_exit(child, code, mode);
                return Ok(1);
            }
            return Ok(0); // clean early exit (CLI tool)
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // Phase 2: liveness (2s-T)
    loop {
        if Instant::now() >= end {
            break;
        }
        if let Some(status) = child.try_wait()? {
            let code = status.code().unwrap_or(1);
            return Ok(i32::from(code != 0));
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // Process is alive — probe if requested
    if let Some(probe_url) = probe {
        return do_probe(child, probe_url, verbose);
    }
    Ok(0)
}

fn report_exit(child: &mut std::process::Child, rc: i32, mode: &str) {
    let label = if rc != 0 { "crashed" } else { "exited" };
    eprintln!("[erebus] selftest: {label} with code {rc}");

    if mode == "cli" {
        if let Some(ref mut stderr) = child.stderr {
            use std::io::Read;
            let mut output = String::new();
            let _ = stderr.read_to_string(&mut output);
            if !output.is_empty() {
                let lines: Vec<&str> = output.lines().collect();
                let tail: Vec<&str> = lines.iter().rev().take(20).rev().copied().collect();
                eprintln!("--- output (last 20 lines) ---");
                for line in &tail {
                    eprintln!("{line}");
                }
            }
        }
    }
}

fn do_probe(child: &mut std::process::Child, probe_url: &str, verbose: bool) -> Result<i32> {
    let deadline = Instant::now() + Duration::from_secs(3);
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .danger_accept_invalid_certs(true)
        .build()
        .context("failed to create HTTP client")?;

    loop {
        if Instant::now() >= deadline {
            break;
        }

        // Check if process died
        if let Ok(Some(_)) = child.try_wait() {
            return Ok(1);
        }

        match client.get(probe_url).send() {
            Ok(resp) if resp.status().is_success() => {
                if verbose {
                    eprintln!("[erebus] selftest: probe {probe_url} → {}", resp.status());
                }
                return Ok(0);
            }
            _ => {}
        }

        std::thread::sleep(Duration::from_millis(300));
    }

    eprintln!("[erebus] selftest: alive but probe {probe_url} failed (not responding)");
    Ok(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_mode_server_python_flask() {
        let meta = serde_json::json!({
            "runtime": "python",
            "entrypoint": ["python3", "app.py"]
        });
        assert_eq!(detect_mode(&meta), "server");
    }

    #[test]
    fn test_detect_mode_server_has_services() {
        let meta = serde_json::json!({
            "services": [{"name": "web"}]
        });
        assert_eq!(detect_mode(&meta), "server");
    }

    #[test]
    fn test_detect_mode_cli_go() {
        let meta = serde_json::json!({
            "runtime": "go",
            "entrypoint": ["/app/myapp"]
        });
        assert_eq!(detect_mode(&meta), "cli");
    }

    #[test]
    fn test_detect_mode_server_node_express() {
        let meta = serde_json::json!({
            "runtime": "node",
            "entrypoint": ["node", "server.js"]
        });
        assert_eq!(detect_mode(&meta), "server");
    }
}
