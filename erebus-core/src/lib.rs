//! `erebus-core` — core library for the x.bin build tool.
//!
//! Provides format parsing, compression, runtime detection, and package manager
//! detection. Used by both the Python CLI (via `PyO3` or subprocess) and the
//! future full-Rust CLI.

pub mod assembler;
pub mod assembly;
pub mod cas;
pub mod chunker;
pub mod compress;
pub mod cron;
pub mod detect;
pub mod dotenv;
pub mod embed;
pub mod encrypt;
pub mod format;
pub mod include;
pub mod layer;
pub mod legacy;
pub mod manifest;
pub mod metadata;
pub mod minify;
pub mod otel;
pub mod paths;
pub mod persistent;
pub mod pkgmgr;
pub mod sisr;
pub mod sisr_header;
pub mod sisr_stage;
pub mod tar;
pub mod treeshake;

#[cfg(feature = "python")]
pub mod python;
