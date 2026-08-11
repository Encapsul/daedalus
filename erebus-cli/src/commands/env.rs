use anyhow::Result;
use clap::Args;
use std::path::PathBuf;

#[derive(Args)]
pub struct EnvArgs {
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: EnvArgs) -> Result<()> {
    let stub = find_binary("erebus-stub", "XBIN_STUB_PATH");
    let crypto = find_binary("erebus-crypto", "XBIN_CRYPTO_PATH");

    if args.json {
        let info = serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "rustc": rustc_version(),
            "stub": stub.map(|p| p.display().to_string()),
            "crypto": crypto.map(|p| p.display().to_string()),
            "arch": std::env::consts::ARCH,
            "os": std::env::consts::OS,
        });
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        eprintln!("erebus {}", env!("CARGO_PKG_VERSION"));
        eprintln!("  arch:   {}", std::env::consts::ARCH);
        eprintln!("  os:     {}", std::env::consts::OS);
        eprintln!(
            "  rustc:  {}",
            rustc_version().unwrap_or_else(|| "unknown".into())
        );
        match &stub {
            Some(p) => eprintln!("  stub:   {}", p.display()),
            None => eprintln!("  stub:   not found"),
        }
        match &crypto {
            Some(p) => eprintln!("  crypto: {}", p.display()),
            None => eprintln!("  crypto: not found"),
        }
    }

    Ok(())
}

fn find_binary(name: &str, env_var: &str) -> Option<PathBuf> {
    if let Ok(path) = std::env::var(env_var) {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    which::which(name).ok()
}

fn rustc_version() -> Option<String> {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
}
