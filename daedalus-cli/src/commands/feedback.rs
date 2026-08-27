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

pub fn run(args: FeedbackArgs) -> Result<()> {
    let repo_url = "https://github.com/Tednoob17/daedalus/issues/new";

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
    fn open(&self, url: &str) -> Result<()>;
}

struct ExternalBrowser {
    cmd: String,
    args: Vec<String>,
}

impl BrowserOpener for ExternalBrowser {
    fn open(&self, url: &str) -> Result<()> {
        std::process::Command::new(&self.cmd)
            .args(&self.args)
            .arg(url)
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to launch browser: {e}"))?;
        Ok(())
    }
}
