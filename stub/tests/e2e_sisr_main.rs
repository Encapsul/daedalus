//! Entry point for the SISR E2E integration tests.
//!
//! Kept as a thin root so the real tests can live in the `e2e_sisr/`
//! directory (Cargo auto-discovers only files directly under `tests/`).

#[path = "e2e_sisr/mod.rs"]
mod e2e_sisr;
