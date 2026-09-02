//! `daedalus-core` — core library for the x.bin build tool.
//!
//! Provides format parsing, compression, runtime detection, and package manager
//! detection. Used by the Rust CLI (`daedalus-cli`) and the launcher stub
//! (`daedalus-stub`).

#![allow(missing_docs)]

pub mod assembler;
pub mod assembly;
pub mod cas;
pub mod chunker;
pub mod compress;
pub mod cron;
pub mod deps;
pub mod detect;
pub mod dotenv;
pub mod embed;
pub mod encrypt;
pub mod format;
pub mod include;
pub mod layer;
pub mod legacy;
pub mod manifest;
pub mod mcp;
pub mod metadata;
pub mod minify;
pub mod paths;
pub mod persistent;
pub mod pkgmgr;
pub mod registry;
pub mod sisr;
pub mod sisr_header;
pub mod sisr_stage;
pub mod tar;
pub mod treeshake;
pub mod universal;

#[cfg(feature = "python")]
pub mod python;
