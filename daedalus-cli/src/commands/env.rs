use anyhow::Result;
use clap::Args;
use std::path::PathBuf;

#[derive(Args)]
pub struct EnvArgs {
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

/// run - run.
/// @args: command arguments
///
/// Description:
///
/// Return: Result containing Result<()>
pub fn run(args: EnvArgs) -> Result<()> {
    let stub = find_binary("daedalus-stub", "DAEDALUS_STUB_PATH");
    let crypto = find_binary("daedalus-crypto", "DAEDALUS_CRYPTO_PATH");

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
        eprintln!("daedalus {}", env!("CARGO_PKG_VERSION"));
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

/// find_binary - find binary.
/// @name: name
/// @env_var: env var
///
/// Description:
///
/// Return: Some(...) if present, None otherwise
fn find_binary(name: &str, env_var: &str) -> Option<PathBuf> {
    if let Ok(path) = std::env::var(env_var) {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    which::which(name).ok()
}

/// rustc_version - rustc version.
///
/// Description:
///
/// Return: Some(...) if present, None otherwise
fn rustc_version() -> Option<String> {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
}
