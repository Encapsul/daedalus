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
}

struct Check {
    name: String,
    ok: bool,
    detail: String,
}

pub fn run(args: DoctorArgs) -> Result<(), Box<dyn std::error::Error>> {
    let checks = vec![
        check_command("python3", &["--version"]),
        check_command("pip", &["--version"]),
        check_command("cargo", &["--version"]),
        check_command("rustc", &["--version"]),
        check_musl_target(),
        check_command("cc", &["--version"]),
        check_command("zstd", &["--version"]),
        check_command("mksquashfs", &["-version"]),
        check_command("node", &["--version"]),
        check_command("deno", &["--version"]),
        check_xbin_stub(),
    ];

    if args.json {
        let items: Vec<_> = checks.iter().map(|c| {
            serde_json::json!({
                "name": c.name,
                "ok": c.ok,
                "detail": c.detail,
            })
        }).collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else {
        let mut all_ok = true;
        for check in &checks {
            let marker = if check.ok { "✓" } else { "✗" };
            let color = if check.ok { "\x1b[32m" } else { "\x1b[31m" };
            eprintln!("  {color}{marker}\x1b[0m {:<20} {}", check.name, check.detail);
            if !check.ok {
                all_ok = false;
            }
        }
        eprintln!();
        if all_ok {
            eprintln!("All checks passed");
        } else {
            return Err("Some checks failed — install missing dependencies".into());
        }
    }

    Ok(())
}

fn check_command(name: &str, args: &[&str]) -> Check {
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
            }
        }
        Err(_) => Check {
            name: name.to_string(),
            ok: false,
            detail: "not found".to_string(),
        },
    }
}

fn check_musl_target() -> Check {
    let output = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let has_musl = stdout.lines().any(|l| l.contains("x86_64-unknown-linux-musl"));
            Check {
                name: "musl target".to_string(),
                ok: has_musl,
                detail: if has_musl {
                    "x86_64-unknown-linux-musl installed".to_string()
                } else {
                    "run: rustup target add x86_64-unknown-linux-musl".to_string()
                },
            }
        }
        _ => Check {
            name: "musl target".to_string(),
            ok: false,
            detail: "rustup not found".to_string(),
        },
    }
}

fn check_xbin_stub() -> Check {
    match which::which("xbin-stub") {
        Ok(path) => Check {
            name: "xbin-stub".to_string(),
            ok: true,
            detail: path.display().to_string(),
        },
        Err(_) => Check {
            name: "xbin-stub".to_string(),
            ok: false,
            detail: "not found — run: make stub".to_string(),
        },
    }
}
