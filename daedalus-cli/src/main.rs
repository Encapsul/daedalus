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
    name = "daedalus",
    version,
    about = "Package any app into a single self-extracting binary",
    long_about = "daedalus compiles any web, server, or CLI application into a\nsingle self-extracting ELF executable.\n\nSupported runtimes: Python, Node.js, Deno, Java, Ruby, .NET/C#,\nGo, PHP, Perl, Binary, Hugo.\n\nExamples:\n  daedalus build ./myapp -o myapp.de\n  daedalus run myapp.de\n  daedalus inspect myapp.de\n  daedalus keygen\n  daedalus sign myapp.de --key ~/.daedalus/keys/*.key\n  daedalus verify myapp.de\n  daedalus doctor\n  daedalus scan .\n  daedalus dashboard\n  daedalus completion bash >> ~/.bashrc\n  daedalus completion zsh >> ~/.zshrc\n  daedalus completion fish > ~/.config/fish/completions/daedalus.fish"
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
    /// Build a .de binary from an app directory
    ///
    /// Examples:
    ///   daedalus build ./myapp -o myapp.de
    ///   daedalus build ./myapp --target aarch64 --squashfs
    ///   daedalus build ./myapp --sign --key ~/.daedalus/keys/*.key
    ///   daedalus build ./myapp --health-port 8081
    ///   daedalus build ./myapp --persist --env-file .env
    Build(Box<commands::build::BuildArgs>),

    /// Execute a .de file
    Run(commands::run::RunArgs),

    /// Inspect a .de file's metadata
    Inspect(commands::inspect::InspectArgs),

    /// Generate Ed25519 signing keys
    Keygen(commands::keygen::KeygenArgs),

    /// Sign a .de file
    Sign(commands::sign::SignArgs),

    /// Verify a .de file's signature
    Verify(commands::verify::VerifyArgs),

    /// Trust a public key
    Trust(commands::trust::TrustArgs),

    /// Scan directories for .de files
    Scan(commands::scan::ScanArgs),

    /// Check system prerequisites
    Doctor(commands::doctor::DoctorArgs),

    /// Clean daedalus cache
    Clean(commands::clean::CleanArgs),

    /// Interactive TUI dashboard showing SISR benchmark & cache status
    Dashboard(commands::dashboard::DashboardArgs),

    /// Test a .de file in an ephemeral sandbox
    Selftest(commands::selftest::SelftestArgs),

    /// Upgrade daedalus to the latest release
    Upgrade(commands::upgrade::UpgradeArgs),

    /// Migrate a legacy .daedalus (v1) to the SISR-enabled v2 format
    Migrate(commands::upgrade_binary::UpgradeBinaryArgs),

    /// Hot-swap a layer in an existing .de binary
    Swap(commands::swap::SwapArgs),

    /// Publish a .de file to a registry
    Publish(commands::publish::PublishArgs),

    /// Push/Pull/List layers in a content-addressable registry
    Registry(commands::registry::RegistryArgs),

    /// Start a local registry server
    Serve(commands::serve::ServeArgs),

    /// Show daedalus environment info
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
        Commands::Migrate(mut args) => {
            if cli.quiet {
                args.quiet = true;
            }
            commands::upgrade_binary::run(args)
        }
        Commands::Publish(args) => commands::publish::run(args),
        Commands::Registry(args) => commands::registry::run(args),
        Commands::Serve(args) => commands::serve::run(args),
        Commands::Env(args) => commands::env::run(args),
        Commands::Completion { shell } => {
            let mut cmd = Cli::command();
            let bin_name = "daedalus";
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
        Commands::Swap(mut args) => {
            if cli.quiet {
                args.quiet = true;
            }
            commands::swap::run(args)
        }
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
    let manual = format!("daedalus {version}");

    // ── Main man page (daedalus.1) ────────────────────────────────────────
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
       DAEDALUS_CACHE_DIR
              Override the cache directory (default: ~/.cache/daedalus).

       DAEDALUS_VERBOSE
              If set, enable verbose output (equivalent to -v).

       XDG_DATA_HOME
              Base directory for daedalus keys and trusted keys.

FILES
       ~/.cache/daedalus/<hash>/rootfs/
              Extracted rootfs for each built binary, keyed by SHA-256 hash.

       ~/.daedalus/trusted-keys/
              Directory of trusted public keys for signature verification.

       ~/.local/share/daedalus/keys/
              Default directory for generated signing keys.

SEE ALSO
       daedalus-build(1), daedalus-sign(1), daedalus-verify(1), daedalus-keygen(1),
       daedalus-inspect(1), daedalus-doctor(1)

AUTHORS
       Written by {author}.

HISTORY
       daedalus was started in 2025 by {author} to solve the problem of
       packaging complex multi-dependency applications into a single
       portable binary.  The Rust CLI (v0.1.0) replaced the legacy
       Python CLI in 2026.

BUGS
        Report bugs at: https://github.com/Tednoob17/daedalus/issues
"
    );
    buffer.extend_from_slice(extra.as_bytes());
    std::fs::write(dir.join("daedalus.1"), &buffer)?;

    // ── Subcommand man pages ──────────────────────────────────────────
    let sub_extras: &[(&str, &str)] = &[
        ("build", "ENVIRONMENT\n       DAEDALUS_CACHE_DIR\n              Override the cache directory.\n"),
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
        ("clean", "FILES\n       ~/.cache/daedalus/\n              The cache directory cleaned by this command.\n"),
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
       daedalus(1)

AUTHORS
       Written by {author}.

HISTORY
       Part of daedalus since v0.1.0.
{extra_footer}"
        );
        buffer.extend_from_slice(shared.as_bytes());
        std::fs::write(dir.join(format!("daedalus-{sub_name}.1")), &buffer)?;
    }

    eprintln!("Generated man pages in {}", dir.display());
    Ok(())
}
