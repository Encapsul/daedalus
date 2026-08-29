//! daedalus-fuzz — high-throughput structure-aware fuzzer for daedalus.

use clap::Parser;
use daedalus_fuzz::{FuzzConfig, FuzzHarness, MutationStrategy, TargetRegistry};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(
    name = "daedalus-fuzz",
    version,
    about = "High-throughput structure-aware fuzzer for daedalus"
)]
struct Args {
    /// Target(s) to fuzz (comma-separated: format,stub,cli,crypto,sisr)
    #[arg(short, long, default_value = "format,stub,cli,crypto,sisr")]
    targets: String,

    /// Maximum number of iterations per worker
    #[arg(long)]
    iterations: Option<u64>,

    /// Maximum duration (e.g., "30m", "2h", "1d")
    #[arg(long)]
    duration: Option<String>,

    /// Number of worker threads (default: CPU count)
    #[arg(short, long)]
    workers: Option<usize>,

    /// Directory for corpus persistence
    #[arg(long)]
    corpus_dir: Option<PathBuf>,

    /// Directory for crash artifacts
    #[arg(long)]
    crash_dir: Option<PathBuf>,

    /// Random seed (default: random)
    #[arg(long)]
    seed: Option<u64>,

    /// Timeout per iteration
    #[arg(long, default_value = "5s")]
    timeout: humantime::Duration,

    /// Corpus save interval
    #[arg(long, default_value = "60s")]
    corpus_interval: humantime::Duration,

    /// Stats print interval
    #[arg(long, default_value = "10s")]
    stats_interval: humantime::Duration,

    /// Mutation strategy
    #[arg(long, value_enum, default_value = "havoc")]
    strategy: MutationStrategyCli,

    /// Minimize crashes automatically
    #[arg(long, default_value = "true")]
    minimize_crashes: bool,

    /// Maximum input size in bytes
    #[arg(long, default_value = "10485760")]
    max_input_size: usize,

    /// Enable cross-over mutations
    #[arg(long, default_value = "true")]
    enable_crossover: bool,

    /// List available targets and exit
    #[arg(long)]
    list_targets: bool,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum MutationStrategyCli {
    BitFlip,
    ByteInsertDelete,
    Structured,
    Crossover,
    Semantic,
    Havoc,
}

impl From<MutationStrategyCli> for MutationStrategy {
    fn from(s: MutationStrategyCli) -> Self {
        match s {
            MutationStrategyCli::BitFlip => MutationStrategy::BitFlip,
            MutationStrategyCli::ByteInsertDelete => MutationStrategy::ByteInsertDelete,
            MutationStrategyCli::Structured => MutationStrategy::Structured,
            MutationStrategyCli::Crossover => MutationStrategy::Crossover,
            MutationStrategyCli::Semantic => MutationStrategy::Semantic,
            MutationStrategyCli::Havoc => MutationStrategy::Havoc,
        }
    }
}

fn parse_duration(s: &str) -> anyhow::Result<Duration> {
    Ok(humantime::parse_duration(s)?)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    if args.list_targets {
        let registry = TargetRegistry::new();
        println!("Available fuzzing targets:");
        for name in registry.all_names() {
            println!("  {}", name);
        }
        return Ok(());
    }

    let target_names: Vec<String> = args
        .targets
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();

    // Validate target names
    let registry = TargetRegistry::new();
    for name in &target_names {
        if registry.get(name).is_none() {
            anyhow::bail!(
                "unknown target: {}. Available: {:?}",
                name,
                registry.all_names()
            );
        }
    }

    let config = FuzzConfig {
        target_names,
        iterations: args.iterations,
        duration: args.duration.as_deref().map(parse_duration).transpose()?,
        workers: args.workers.unwrap_or_else(num_cpus::get),
        corpus_dir: args.corpus_dir,
        crash_dir: args.crash_dir,
        seed: args.seed.unwrap_or_else(rand::random),
        timeout_per_iteration: args.timeout.into(),
        corpus_save_interval: args.corpus_interval.into(),
        stats_print_interval: args.stats_interval.into(),
        mutation_strategy: args.strategy.into(),
        minimize_crashes: args.minimize_crashes,
        max_input_size: args.max_input_size,
        enable_cross_over: args.enable_crossover,
    };

    println!("daedalus-fuzz starting...");
    println!("  targets: {:?}", config.target_names);
    println!("  workers: {}", config.workers);
    println!("  seed: {}", config.seed);
    println!("  strategy: {:?}", config.mutation_strategy);

    let harness = FuzzHarness::new(config);
    let stats = harness.run().await?;

    println!("\n=== Fuzzing complete ===");
    println!("  iterations: {}", stats.iterations);
    println!("  crashes:    {}", stats.crashes);
    println!("  panics:     {}", stats.panics);
    println!("  timeouts:   {}", stats.timeouts);
    println!("  corpus:     {}", stats.corpus_size);
    println!("  unique crashes: {}", stats.unique_crashes.len());

    if stats.crashes > 0 {
        println!("\nCrash summary:");
        for (sig, count) in &stats.unique_crashes {
            println!("  {} x {}", sig, count);
        }
        std::process::exit(1);
    }

    Ok(())
}
