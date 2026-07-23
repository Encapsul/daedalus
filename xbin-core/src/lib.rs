//! `xbin-core` — core library for the x.bin build tool.
//!
//! Provides format parsing, compression, runtime detection, and package manager
//! detection. Used by both the Python CLI (via `PyO3` or subprocess) and the
//! future full-Rust CLI.

pub mod assembly;
pub mod compress;
pub mod cron;
pub mod detect;
pub mod dotenv;
pub mod encrypt;
pub mod format;
pub mod include;
pub mod layers;
pub mod minify;
pub mod otel;
pub mod persistent;
pub mod pkgmgr;
pub mod tar;
pub mod treeshake;

#[cfg(feature = "python")]
pub mod python;
