use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct FeedbackArgs {
    /// Open GitHub Issues page in browser
    #[arg(long)]
    pub browser: bool,

    /// GitHub issue title (auto-filled)
    #[arg(long)]
    pub title: Option<String>,
}

/// run - open the daedalus GitHub issues page for feedback.
/// @args: command arguments
///
/// Description:
/// Prints the GitHub issues URL, optionally opening it in the default browser.
///
/// Return: Result containing Result<()>
pub fn run(args: FeedbackArgs) -> Result<()> {
    let repo_url = "https://github.com/Encapsul/daedalus/issues/new";

    let mut url = repo_url.to_string();
    if let Some(title) = &args.title {
        url.push_str(&format!("?title={}", urlencoding::encode(title)));
    }

    if args.browser {
        if let Some(opener) = open_browser() {
            opener.open(&url)?;
            eprintln!("Opened feedback page in browser: {url}");
        } else {
            eprintln!("Could not detect a browser. Visit: {url}");
        }
    } else {
        eprintln!("Report issues at: {url}");
        eprintln!("\nOr run with --browser to open in your default browser.");
    }

    Ok(())
}

/// open_browser - detect and return a platform-specific browser opener.
///
/// Description:
/// Returns xdg-open on Linux, open on macOS, or cmd /C start on Windows.
///
/// Return: Some(...) if present, None otherwise
fn open_browser() -> Option<Box<dyn BrowserOpener>> {
    let cmd = if cfg!(target_os = "linux") {
        Some(("xdg-open", vec![]))
    } else if cfg!(target_os = "macos") {
        Some(("open", vec![]))
    } else if cfg!(target_os = "windows") {
        Some(("cmd", vec!["/C".to_string(), "start".to_string()]))
    } else {
        None
    };

    cmd.map(|(name, args)| {
        Box::new(ExternalBrowser {
            cmd: name.to_string(),
            args,
        }) as Box<dyn BrowserOpener>
    })
}

trait BrowserOpener {
    /// open - open a URL in the default browser.
    /// @url: URL
    ///
    /// Description:
    /// Launches the system browser with the given URL.
    ///
    /// Return: Result containing Result<()>
    fn open(&self, url: &str) -> Result<()>;
}

struct ExternalBrowser {
    cmd: String,
    args: Vec<String>,
}

impl BrowserOpener for ExternalBrowser {
    /// open - open a URL using the configured external browser command.
    /// @url: URL
    ///
    /// Description:
    /// Spawns the browser command as a detached child process.
    ///
    /// Return: Result containing Result<()>
    fn open(&self, url: &str) -> Result<()> {
        std::process::Command::new(&self.cmd)
            .args(&self.args)
            .arg(url)
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to launch browser: {e}"))?;
        Ok(())
    }
}
