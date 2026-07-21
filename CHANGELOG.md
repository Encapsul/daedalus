# Changelog

All notable changes to x.bin will be documented in this file.

Format based on [Keep a Changelog](https://keepachangelog.com/).

## [0.3.0] - 2026-07-21

### Added
- **Rust CLI** (`xbin` binary) — complete rewrite of the CLI in Rust
  - `xbin build` — package any app into a self-extracting ELF binary
  - `xbin inspect` — read metadata from a `.xbin` file
  - `xbin scan` — find `.xbin` files recursively
  - `xbin sign` / `xbin verify` — Ed25519 signing and verification
  - `xbin keygen` — generate signing keypairs
  - `xbin trust` — manage trusted public keys
  - `xbin doctor` — check system prerequisites (`--strict` mode)
  - `xbin env` — show environment info
  - `xbin clean` — remove cache
  - `xbin completion` — shell completions (bash, zsh, fish, elvish, powershell)
  - `xbin man` — generate Unix man pages
- **`.xbin.toml`** config file — set default build/package options in your app directory
- **`--dry-run`** flag on build, inspect, scan — preview without building
- **`--verbose`** global flag — detailed output on any command
- **`--strict`** on `xbin doctor` — fail on missing required tools
- **`anyhow`** — structured error messages with context
- **`human-panic`** — user-friendly crash reports
- **README.md** — full project documentation with badges, tables, examples
- **10 integration tests** for the Rust CLI
- **CI** — added xbin-cli clippy + build steps

### Changed
- Version bumped from 0.2.9 to 0.3.0
- `xbin-stub` cross-compile target: `aarch64-unknown-linux-musl` → `aarch64-unknown-linux-gnu`
- `xbin doctor` is non-fatal by default (use `--strict` to enforce)
- Release workflow `needs` reduced to 3/4 jobs (skip slow macos-x64)

### Fixed
- `find_binary` now searches workspace-level `target/` directory
- Python CLI `--version` reads from `pyproject.toml` (was hardcoded "0.1.0")
- Version centralization: single source of truth in `pyproject.toml`

## [0.2.9] - 2026-07-21

### Fixed
- `xbin-stub` and `xbin-crypto` marked as optional in doctor checks

## [0.2.8] - 2026-07-21

### Added
- Python CLI shell completion (bash, zsh, fish)
- Python CLI `--strict` flag for doctor
- `find_binary` searches workspace-level `target/` directory

## [0.2.7] - 2026-07-21

### Added
- Multi-arch GitHub releases (Linux x64/arm64, macOS x64/arm64)
- Release workflow triggered on GitHub release publish
- SSH signing for all commits and tags
- `xbin doctor` health check command
- `xbin env` environment info command
