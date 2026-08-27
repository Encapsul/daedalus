# CLAUDE.md

This file provides guidance when working with the daedalus codebase.

## Project Overview

**daedalus packages any application into a single self-extracting binary.**

daedalus compiles any web, server, or CLI application into a single self-contained
executable. The binary format (`[stub][payload][metadata][footer]`) is a universal
executable artifact — capable of transporting an application, a microservice, or a
plugin as a single portable unit. Supported runtimes: Python, Node.js, Deno, Java,
Ruby, .NET/C#, Go, PHP, Perl, Hugo, Binary.

## Architecture

- `daedalus-core/` — Core library (format, detect, compress, encrypt, integrity, verify, assembly, tar, SISR)
- `daedalus-cli/` — Rust CLI tool (clap-based, all commands)
- `daedalus-stub/` — Self-extracting launcher (Linux ELF, macOS Mach-O, Windows PE)

## Build Commands

```bash
make preflight    # Check toolchain
make stub         # Build stub for current arch
make build        # Full pipeline: preflight + stub + cargo build
make test         # Full verification: lint + fmt + clippy + cargo test
make lint         # Run all linters (cargo fmt + clippy)
make fmt          # Auto-format all code
```

## Verification Loop (MANDATORY)

Before finishing any code change, run:
1. `cargo fmt --check`
2. `cargo clippy -p daedalus-core --all-targets -- -D warnings`
3. `cargo clippy -p daedalus-stub --all-targets -- -D warnings`
4. `cargo clippy -p daedalus-cli --all-targets -- -D warnings`
5. `cargo test --workspace`
6. `cargo build --release && ./target/release/daedalus build examples/hello-web -o /tmp/test.de && ./target/release/daedalus inspect /tmp/test.de`

## Security Rules

- No `unsafe` in `daedalus-core/` (only `stub/src/main.rs`)
- All `unsafe` blocks must have `SAFETY` comments
- Ed25519 keys must have the Ed25519 bit set (CVE-2023-48022)
- No hardcoded secrets anywhere in the codebase
- Use `cargo audit` periodically for dependency vulnerabilities

## Linting & Formatting

### Rust
- `cargo fmt --check` — formatting
- `cargo clippy --all-targets -- -D warnings` — linting
- Edition 2021, `opt-level = "z"`, LTO, strip, `panic = "abort"`

### CI (GitHub Actions)
- `.github/workflows/ci.yml` runs on push/PR to main
- Jobs: preflight, rust (fmt + clippy per crate), test (cargo test --workspace), build (end-to-end)
- PRs that fail CI cannot be merged

## Benchmarking

Benchmarks in `benchmarks/` measure:
- Build time (seconds)
- Output size (MB)
- Peak RSS (MB)
- Cold/warm start time
- Native vs daedalus comparison

Run: `bash benchmarks/run.sh`

Machine specs affect results:
- Xeon 32 cores: < 25s build
- Typical laptop: < 60s build
- Constrained hardware: < 2min build

## External References

- [clig.dev](https://clig.dev) — CLI design conventions
- [POSIX.1-2017 Ch.12](https://pubs.opengroup.org/onlinepubs/9799919797/) — Shell & Utilities
- [ANSSI-Rust](https://anssi-fr.github.io/rust-guide/) — Rust security guidelines
- [Google Doc Style](https://developers.google.com/style) — Documentation style
- [12-Factor CLI](https://medium.com/@jdxcode/12-factor-cli-apps-dd3c227a0e46) — CLI best practices
