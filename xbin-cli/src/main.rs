mod commands;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "xbin",
    version,
    about = "Package any app into a single self-extracting binary",
    long_about = "x.bin compiles any web, server, or CLI application into a\nsingle self-extracting ELF executable.\n\nSupported runtimes: Python, Node.js, Deno, Java, Ruby, .NET/C#,\nGo, PHP, Perl, Binary, Hugo.\n\nExamples:\n  xbin build ./myapp -o myapp.xbin\n  xbin inspect myapp.xbin\n  xbin keygen\n  xbin sign myapp.xbin --key ~/.xbin/keys/*.key\n  xbin verify myapp.xbin\n  xbin doctor\n  xbin scan ."
)]
struct Cli {
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
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build(args) => commands::build::run(args)?,
        Commands::Inspect(args) => commands::inspect::run(args)?,
        Commands::Keygen(args) => commands::keygen::run(args)?,
        Commands::Sign(args) => commands::sign::run(args)?,
        Commands::Verify(args) => commands::verify::run(args)?,
        Commands::Trust(args) => commands::trust::run(args)?,
        Commands::Scan(args) => commands::scan::run(args)?,
        Commands::Doctor(args) => commands::doctor::run(args)?,
        Commands::Clean(args) => commands::clean::run(args)?,
        Commands::Env(args) => commands::env::run(args)?,
    }

    Ok(())
}
