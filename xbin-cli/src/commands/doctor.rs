use anyhow::Result;
use clap::Args;
use std::process::Command;

#[derive(Args)]
pub struct DoctorArgs {
    /// Output as JSON
    #[arg(long)]
    pub json: bool,

    /// Quiet output
    #[arg(short, long)]
    pub quiet: bool,

    /// Exit with error if any check fails
    #[arg(long)]
    pub strict: bool,

    /// Attempt to auto-install missing prerequisites
    #[arg(long)]
    pub r#fix: bool,

    /// Skip confirmation prompt for `--fix`
    #[arg(short, long)]
    pub force: bool,
}

struct Check {
    name: String,
    ok: bool,
    detail: String,
    optional: bool,
}

pub fn run(args: DoctorArgs) -> Result<()> {
    let mut checks = vec![
        check_command("python3", &["--version"], false),
        check_command("pip", &["--version"], false),
        check_command("cargo", &["--version"], false),
        check_command("rustc", &["--version"], false),
        check_musl_target(false),
        check_command("cc", &["--version"], false),
        check_command("zstd", &["--version"], false),
        check_xbin_stub(false),
        // Optional checks
        check_command("node", &["--version"], true),
        check_command("deno", &["--version"], true),
        check_command("mksquashfs", &["-version"], true),
        check_python_cryptography(),
    ];

    // Auto-fix missing prerequisites
    if args.r#fix {
        if !args.force {
            eprint!("This will attempt to install missing dependencies. Continue? [y/N] ");
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if !input.trim().eq_ignore_ascii_case("y") {
                eprintln!("Aborted");
                return Ok(());
            }
        }

        for check in &mut checks {
            if !check.ok && !check.optional {
                let fix_result = attempt_fix(&check.name);
                match fix_result {
                    Ok(msg) => {
                        check.ok = true;
                        check.detail = msg;
                    }
                    Err(e) => {
                        check.detail = format!("fix failed: {e}");
                    }
                }
            }
        }
    }

    if args.json {
        let items: Vec<_> = checks
            .iter()
            .map(|c| {
                serde_json::json!({
                    "name": c.name,
                    "ok": c.ok,
                    "detail": c.detail,
                    "optional": c.optional,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else {
        let mut all_ok = true;
        for check in &checks {
            let marker = if check.ok { "✓" } else { "✗" };
            let color = if check.ok { "\x1b[32m" } else { "\x1b[31m" };
            let optional_tag = if check.optional { " (optional)" } else { "" };
            if !args.quiet || !check.ok {
                eprintln!(
                    "  {color}{marker}\x1b[0m {:<20} {}{optional_tag}",
                    check.name, check.detail
                );
            }
            if !check.ok && !check.optional {
                all_ok = false;
            }
        }
        if !args.quiet {
            eprintln!();
            if all_ok {
                eprintln!("All checks passed");
            } else if args.strict {
                anyhow::bail!("Some checks failed — install missing dependencies");
            } else {
                eprintln!("Some checks failed (non-fatal, use --strict to enforce)");
            }
        } else if !all_ok && args.strict {
            anyhow::bail!("Some checks failed");
        }
    }

    Ok(())
}

fn check_command(name: &str, args: &[&str], optional: bool) -> Check {
    match Command::new(name).args(args).output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let version = if output.status.success() {
                stdout.trim().lines().next().unwrap_or("unknown")
            } else {
                stderr.trim().lines().next().unwrap_or("unknown")
            };
            Check {
                name: name.to_string(),
                ok: output.status.success(),
                detail: version.to_string(),
                optional,
            }
        }
        Err(_) => Check {
            name: name.to_string(),
            ok: false,
            detail: "not found".to_string(),
            optional,
        },
    }
}

fn check_musl_target(optional: bool) -> Check {
    let output = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let has_musl = stdout
                .lines()
                .any(|l| l.contains("x86_64-unknown-linux-musl"));
            Check {
                name: "musl target".to_string(),
                ok: has_musl,
                detail: if has_musl {
                    "x86_64-unknown-linux-musl installed".to_string()
                } else {
                    "run: rustup target add x86_64-unknown-linux-musl".to_string()
                },
                optional,
            }
        }
        _ => Check {
            name: "musl target".to_string(),
            ok: false,
            detail: "rustup not found".to_string(),
            optional,
        },
    }
}

fn check_xbin_stub(optional: bool) -> Check {
    match which::which("xbin-stub") {
        Ok(path) => Check {
            name: "xbin-stub".to_string(),
            ok: true,
            detail: path.display().to_string(),
            optional,
        },
        Err(_) => Check {
            name: "xbin-stub".to_string(),
            ok: false,
            detail: "not found (optional)".to_string(),
            optional: true, // always optional
        },
    }
}

fn check_python_cryptography() -> Check {
    let output = Command::new("python3")
        .args(["-c", "import cryptography; print(cryptography.__version__)"])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let ver = String::from_utf8_lossy(&o.stdout);
            Check {
                name: "cryptography".to_string(),
                ok: true,
                detail: ver.trim().to_string(),
                optional: true,
            }
        }
        _ => Check {
            name: "cryptography".to_string(),
            ok: false,
            detail: "pip install cryptography".to_string(),
            optional: true,
        },
    }
}

fn attempt_fix(name: &str) -> Result<String> {
    match name {
        "musl target" => {
            let status = Command::new("rustup")
                .args(["target", "add", "x86_64-unknown-linux-musl"])
                .status()?;
            if status.success() {
                Ok("installed via rustup".into())
            } else {
                anyhow::bail!("rustup target add failed");
            }
        }
        "cryptography" => {
            let status = Command::new("pip")
                .args(["install", "cryptography"])
                .status()?;
            if status.success() {
                Ok("installed via pip".into())
            } else {
                anyhow::bail!("pip install failed");
            }
        }
        _ => anyhow::bail!("no automatic fix available for '{name}'"),
    }
}
