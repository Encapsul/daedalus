mod commands;
mod error;
mod remote_cache;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{
    generate,
    shells::{Bash, Elvish, Fish, PowerShell, Zsh},
};
use std::io;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "erebus",
    version,
    about = "Package any app into a single self-extracting binary",
    long_about = "x.bin compiles any web, server, or CLI application into a\nsingle self-extracting ELF executable.\n\nSupported runtimes: Python, Node.js, Deno, Java, Ruby, .NET/C#,\nGo, PHP, Perl, Binary, Hugo.\n\nExamples:\n  erebus build ./myapp -o myapp.erebus\n  erebus run myapp.erebus\n  erebus inspect myapp.erebus\n  erebus keygen\n  erebus sign myapp.erebus --key ~/.erebus/keys/*.key\n  erebus verify myapp.erebus\n  erebus doctor\n  erebus scan .\n  erebus dashboard\n  erebus completion bash >> ~/.bashrc\n  erebus completion zsh >> ~/.zshrc\n  erebus completion fish > ~/.config/fish/completions/erebus.fish"
)]
struct Cli {
    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Disable colored output (also respects `NO_COLOR` env)
    #[arg(long, global = true)]
    no_color: bool,

    /// Suppress non-error output
    #[arg(short, long, global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build a .erebus binary from an app directory
    ///
    /// Examples:
    ///   erebus build ./myapp -o myapp.erebus
    ///   erebus build ./myapp --target aarch64 --squashfs
    ///   erebus build ./myapp --sign --key ~/.erebus/keys/*.key
    ///   erebus build ./myapp --encrypt --key ~/.erebus/keys/*.key
    ///     (--encrypt is obfuscation-only: the decryption key is embedded in
    ///      the metadata, so a determined attacker can always extract the app)
    ///   erebus build ./myapp --health-port 8081
    ///   erebus build ./myapp --persist --env-file .env
    Build(Box<commands::build::BuildArgs>),

    /// Execute a .erebus file
    Run(commands::run::RunArgs),

    /// Inspect a .erebus file's metadata
    Inspect(commands::inspect::InspectArgs),

    /// Generate Ed25519 signing keys
    Keygen(commands::keygen::KeygenArgs),

    /// Sign a .erebus file
    Sign(commands::sign::SignArgs),

    /// Verify a .erebus file's signature
    Verify(commands::verify::VerifyArgs),

    /// Trust a public key
    Trust(commands::trust::TrustArgs),

    /// Scan directories for .erebus files
    Scan(commands::scan::ScanArgs),

    /// Check system prerequisites
    Doctor(commands::doctor::DoctorArgs),

    /// Clean erebus cache
    Clean(commands::clean::CleanArgs),

    /// Interactive TUI dashboard showing SISR benchmark & cache status
    Dashboard(commands::dashboard::DashboardArgs),

    /// Test a .erebus file in an ephemeral sandbox
    Selftest(commands::selftest::SelftestArgs),

    /// Upgrade x.bin to the latest release
    Upgrade(commands::upgrade::UpgradeArgs),

    /// Migrate a legacy .erebus (v1) to the SISR-enabled v2 format
    UpgradeBinary(commands::upgrade_binary::UpgradeBinaryArgs),

    /// Publish a .erebus file to a registry
    Publish(commands::publish::PublishArgs),

    /// Show erebus environment info
    Env(commands::env::EnvArgs),

    /// Generate shell completion scripts
    Completion {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Generate man pages (to a directory)
    Man {
        /// Output directory for man pages
        #[arg(default_value = ".")]
        dir: std::path::PathBuf,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Shell {
    Bash,
    Zsh,
    Fish,
    Elvish,
    PowerShell,
}

fn main() -> ExitCode {
    // Hide source locations in panic messages for security (no file/line info leakage)
    human_panic::setup_panic!();

    let cli = Cli::parse();

    // Respect --no-color flag and NO_COLOR env var
    if cli.no_color || std::env::var("NO_COLOR").is_ok() {
        std::env::set_var("NO_COLOR", "1");
    }

    let effective_verbose = cli.verbose && !cli.quiet;

    let result = match cli.command {
        Commands::Build(args) => commands::build::run(*args, effective_verbose),
        Commands::Run(args) => commands::run::run(args),
        Commands::Inspect(args) => commands::inspect::run(args),
        Commands::Keygen(mut args) => {
            if cli.quiet {
                args.quiet = true;
            }
            commands::keygen::run(args)
        }
        Commands::Sign(mut args) => {
            if cli.quiet {
                args.quiet = true;
            }
            commands::sign::run(args)
        }
        Commands::Verify(mut args) => {
            if cli.quiet {
                args.quiet = true;
            }
            commands::verify::run(args)
        }
        Commands::Trust(mut args) => {
            if cli.quiet {
                args.quiet = true;
            }
            commands::trust::run(args)
        }
        Commands::Scan(args) => commands::scan::run(args),
        Commands::Doctor(args) => commands::doctor::run(args),
        Commands::Clean(args) => commands::clean::run(args),
        Commands::Dashboard(args) => commands::dashboard::run(&args),
        Commands::Selftest(mut args) => {
            if cli.quiet {
                args.verbose = false;
            }
            commands::selftest::run(args)
        }
        Commands::Upgrade(mut args) => {
            if cli.quiet {
                args.verbose = false;
            }
            commands::upgrade::run(args)
        }
        Commands::UpgradeBinary(mut args) => {
            if cli.quiet {
                args.quiet = true;
            }
            commands::upgrade_binary::run(args)
        }
        Commands::Publish(args) => commands::publish::run(args),
        Commands::Env(args) => commands::env::run(args),
        Commands::Completion { shell } => {
            let mut cmd = Cli::command();
            let bin_name = "erebus";
            match shell {
                Shell::Bash => generate(Bash, &mut cmd, bin_name, &mut io::stdout()),
                Shell::Zsh => generate(Zsh, &mut cmd, bin_name, &mut io::stdout()),
                Shell::Fish => generate(Fish, &mut cmd, bin_name, &mut io::stdout()),
                Shell::Elvish => generate(Elvish, &mut cmd, bin_name, &mut io::stdout()),
                Shell::PowerShell => generate(PowerShell, &mut cmd, bin_name, &mut io::stdout()),
            }
            Ok(())
        }
        Commands::Man { dir } => generate_man_pages(&dir),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Error: {err:#}");
            error::exit_code_for_error(&err)
        }
    }
}

fn generate_man_pages(dir: &std::path::Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;

    let cmd = Cli::command();
    let version = env!("CARGO_PKG_VERSION");
    let author = "Ted Kouhouenou <ted.sig42@tutamail.com>";
    let manual = format!("x.bin {version}");

    // ── Main man page (erebus.1) ────────────────────────────────────────
    let man = clap_mangen::Man::new(cmd.clone())
        .section("1")
        .manual(&manual)
        .source(author);
    let mut buffer = Vec::new();
    man.render(&mut buffer)?;

    let extra = format!(
        r"
EXIT STATUS
       0      Success.

       1      General error (invalid arguments, build failure, I/O error).

       2      Lint or verification error (signature mismatch, corrupt file).

       3      Data error (parse failure, invalid format).

       4      Permission error (insufficient privileges, file not writable).

       5      Not found (file, directory, or dependency missing).

ENVIRONMENT
       ERE_CACHE_DIR
              Override the cache directory (default: ~/.cache/erebus).

       ERE_VERBOSE
              If set, enable verbose output (equivalent to -v).

       XDG_DATA_HOME
              Base directory for erebus keys and trusted keys.

FILES
       ~/.cache/erebus/<hash>/rootfs/
              Extracted rootfs for each built binary, keyed by SHA-256 hash.

       ~/.erebus/trusted-keys/
              Directory of trusted public keys for signature verification.

       ~/.local/share/erebus/keys/
              Default directory for generated signing keys.

SEE ALSO
       erebus-build(1), erebus-sign(1), erebus-verify(1), erebus-keygen(1),
       erebus-inspect(1), erebus-doctor(1)

AUTHORS
       Written by {author}.

HISTORY
       x.bin was started in 2025 by {author} to solve the problem of
       packaging complex multi-dependency applications into a single
       portable binary.  The Rust CLI (v0.1.0) replaced the legacy
       Python CLI in 2026.

BUGS
        Report bugs at: https://github.com/Tednoob17/erebus/issues
"
    );
    buffer.extend_from_slice(extra.as_bytes());
    std::fs::write(dir.join("erebus.1"), &buffer)?;

    // ── Subcommand man pages ──────────────────────────────────────────
    let sub_extras: &[(&str, &str)] = &[
        ("build", "ENVIRONMENT\n       ERE_CACHE_DIR\n              Override the cache directory.\n"),
        ("run", ""),
        ("sign", "ENVIRONMENT\n       XDG_DATA_HOME\n              Base directory for key lookup.\n"),
        ("verify", "ENVIRONMENT\n       XDG_DATA_HOME\n              Base directory for trusted key lookup.\n"),
        ("keygen", "ENVIRONMENT\n       XDG_DATA_HOME\n              Base directory for key storage.\n"),
        ("doctor", ""),
        ("selftest", ""),
        ("upgrade", ""),
        ("upgrade-binary", "ENVIRONMENT\n       XDG_DATA_HOME\n              Base directory for key lookup.\n"),
        ("inspect", ""),
        ("scan", ""),
        ("clean", "FILES\n       ~/.cache/erebus/\n              The cache directory cleaned by this command.\n"),
        ("dashboard", ""),
        ("env", ""),
        ("trust", "ENVIRONMENT\n       XDG_DATA_HOME\n              Base directory for trusted key storage.\n"),
        ("completion", ""),
        ("man", ""),
    ];

    for sub in cmd.get_subcommands() {
        let sub_name = sub.get_name().to_owned();
        let sub = sub.clone();

        let man = clap_mangen::Man::new(sub)
            .section("1")
            .manual(&manual)
            .source(author);
        let mut buffer = Vec::new();
        man.render(&mut buffer)?;

        let extra_footer = sub_extras
            .iter()
            .find(|(name, _)| *name == sub_name)
            .map(|(_, e)| *e)
            .unwrap_or("");

        let shared = format!(
            r"
EXIT STATUS
       0      Success.

       1      General error.

       2      Verification error.

       3      Data error.

       4      Permission error.

       5      Not found.

SEE ALSO
       erebus(1)

AUTHORS
       Written by {author}.

HISTORY
       Part of x.bin since v0.1.0.
{extra_footer}"
        );
        buffer.extend_from_slice(shared.as_bytes());
        std::fs::write(dir.join(format!("erebus-{sub_name}.1")), &buffer)?;
    }

    eprintln!("Generated man pages in {}", dir.display());
    Ok(())
}
