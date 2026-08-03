//! SISR (Self-Interpreting Runtime) update engine.
//!
//! Rebuilds a `.xbin` in place from a delta manifest by reusing unchanged
//! chunks from the running binary and fetching the rest. See
//! `docs/src/architecture/runtime-launcher.md` for the launcher integration.

pub mod engine;
pub mod health;
pub mod resilience;
pub mod swap;
