//! daedalus-fuzz — high-throughput structure-aware fuzzer for the daedalus ecosystem.
//!
//! Targets:
//! - .de binary format (footer, metadata, payload extraction)
//! - Stub launcher (extraction, exec resolution, path traversal)
//! - CLI argument parsing (all subcommands)
//! - Encryption/decryption path (AES-256-GCM)
//! - SISR manifest parsing
//!
//! Run: `cargo run -p daedalus-fuzz -- [SUBCOMMAND]`

pub mod cli_fuzz;
pub mod crypto_fuzz;
pub mod format_fuzz;
pub mod harness;
pub mod registry_fuzz;
pub mod serve_fuzz;
pub mod sisr_fuzz;
pub mod stub_fuzz;

// Single source of truth for shared fuzzing types lives in `harness`;
// historical duplicates here caused trait-mismatch bugs after refactors.
pub use harness::{
    CorpusEntry, CrashCase, FuzzConfig, FuzzHarness, FuzzStats, FuzzTarget, GlobalStats,
    MutationStrategy, TargetRegistry,
};
