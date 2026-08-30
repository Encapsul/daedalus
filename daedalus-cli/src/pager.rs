use anyhow::{Context, Result};
use std::env;
use std::io::{self, IsTerminal, Write};
use std::process::{Command, Stdio};

/// Page `content` through the user's pager if stdout is a TTY and a pager is
/// available. Honors `NO_COLOR`/`--plain` implicitly: if the caller already
/// decided not to page, it shouldn't call this.
///
/// Pager selection order:
/// 1. `DAEDALUS_PAGER` env var
/// 2. `PAGER` env var
/// 3. `less` if available
/// 4. `more` if available
/// 5. No pager (print to stdout directly)
pub fn page(content: &str) -> Result<()> {
    if std::env::var("NO_COLOR").is_ok() {
        print!("{content}");
        io::stdout().flush()?;
        return Ok(());
    }

    let pager = env::var("DAEDALUS_PAGER")
        .or_else(|_| env::var("PAGER"))
        .unwrap_or_else(|_| detect_pager());

    if pager.is_empty() || !io::stdout().is_terminal() {
        print!("{content}");
        io::stdout().flush()?;
        return Ok(());
    }

    let mut child = Command::new(&pager)
        .env("LESS", "FRX")
        .env("MORE", "-")
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("failed to spawn pager: {pager}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(content.as_bytes())?;
    }

    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("pager exited with status: {status:?}");
    }

    Ok(())
}

fn detect_pager() -> String {
    ["less", "more"]
        .iter()
        .find(|&&p| which::which(p).is_ok())
        .map_or_else(|| "less".to_string(), |&p| p.to_string())
}
