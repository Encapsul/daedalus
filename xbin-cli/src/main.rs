mod commands;

use anyhow::Result;
use clap::{Parser, Subcommand, CommandFactory};
use clap_complete::{generate, shells::{Bash, Elvish, Fish, PowerShell, Zsh}};
use std::io;

#[derive(Parser)]
#[command(
    name = "xbin",
    version,
    about = "Package any app into a single self-extracting binary",
    long_about = "x.bin compiles any web, server, or CLI application into a\nsingle self-extracting ELF executable.\n\nSupported runtimes: Python, Node.js, Deno, Java, Ruby, .NET/C#,\nGo, PHP, Perl, Binary, Hugo.\n\nExamples:\n  xbin build ./myapp -o myapp.xbin\n  xbin inspect myapp.xbin\n  xbin keygen\n  xbin sign myapp.xbin --key ~/.xbin/keys/*.key\n  xbin verify myapp.xbin\n  xbin doctor\n  xbin scan .\n  xbin completion bash >> ~/.bashrc\n  xbin completion zsh >> ~/.zshrc\n  xbin completion fish > ~/.config/fish/completions/xbin.fish"
)]
struct Cli {
    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build a .xbin binary from an app directory
    Build(commands::build::BuildArgs),

    /// Inspect a .xbin file's metadata
    Inspect(commands::inspect::InspectArgs),

    /// Generate Ed25519 signing keys
    Keygen(commands::keygen::KeygenArgs),

    /// Sign a .xbin file
    Sign(commands::sign::SignArgs),

    /// Verify a .xbin file's signature
    Verify(commands::verify::VerifyArgs),

    /// Trust a public key
    Trust(commands::trust::TrustArgs),

    /// Scan directories for .xbin files
    Scan(commands::scan::ScanArgs),

    /// Check system prerequisites
    Doctor(commands::doctor::DoctorArgs),

    /// Clean xbin cache
    Clean(commands::clean::CleanArgs),

    /// Show xbin environment info
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

fn main() -> Result<()> {
    human_panic::setup_panic!();

    let cli = Cli::parse();

    match cli.command {
        Commands::Build(args) => commands::build::run(args, cli.verbose),
        Commands::Inspect(args) => commands::inspect::run(args),
        Commands::Keygen(args) => commands::keygen::run(args),
        Commands::Sign(args) => commands::sign::run(args),
        Commands::Verify(args) => commands::verify::run(args),
        Commands::Trust(args) => commands::trust::run(args),
        Commands::Scan(args) => commands::scan::run(args),
        Commands::Doctor(args) => commands::doctor::run(args),
        Commands::Clean(args) => commands::clean::run(args),
        Commands::Env(args) => commands::env::run(args),
        Commands::Completion { shell } => {
            let mut cmd = Cli::command();
            let bin_name = "xbin";
            match shell {
                Shell::Bash => generate(Bash, &mut cmd, bin_name, &mut io::stdout()),
                Shell::Zsh => generate(Zsh, &mut cmd, bin_name, &mut io::stdout()),
                Shell::Fish => generate(Fish, &mut cmd, bin_name, &mut io::stdout()),
                Shell::Elvish => generate(Elvish, &mut cmd, bin_name, &mut io::stdout()),
                Shell::PowerShell => generate(PowerShell, &mut cmd, bin_name, &mut io::stdout()),
            }
            Ok(())
        }
        Commands::Man { dir } => {
            generate_man_pages(&dir)
        }
    }
}

fn generate_man_pages(dir: &std::path::PathBuf) -> Result<()> {
    std::fs::create_dir_all(dir)?;

    let cmd = Cli::command();

    // Generate man page for the main command
    let man = clap_mangen::Man::new(cmd.clone())
        .section("1")
        .manual("x.bin 0.3.0")
        .source("Ted Kouhouenou <ted.sig42@tutamail.com>");
    let mut buffer = Vec::new();
    man.render(&mut buffer)?;
    let man_path = dir.join("xbin.1");
    std::fs::write(&man_path, &buffer)?;

    // Generate man pages for subcommands
    for sub in cmd.get_subcommands() {
        let sub_name = sub.get_name();
        let mut sub_cmd = cmd.clone();
        sub_cmd = sub_cmd.subcommand(sub.clone());

        let man = clap_mangen::Man::new(sub_cmd)
            .section("1")
            .manual("x.bin 0.3.0")
            .source("Ted Kouhouenou <ted.sig42@tutamail.com>");
        let mut buffer = Vec::new();
        man.render(&mut buffer)?;
        let man_path = dir.join(format!("xbin-{}.1", sub_name));
        std::fs::write(&man_path, &buffer)?;
    }

    eprintln!("Generated man pages in {}", dir.display());
    Ok(())
}
