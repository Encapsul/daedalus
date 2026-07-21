# Changelog

All notable changes to x.bin will be documented in this file.

Format based on [Keep a Changelog](https://keepachangelog.com/).

## [0.3.0] - 2026-07-21

### Added
- **Full Rust CLI** — zero Python dependency at runtime
  - `xbin build` — package any app into self-extracting ELF
  - `xbin inspect` — read metadata from .xbin
  - `xbin scan` — find .xbin files recursively
  - `xbin sign` / `xbin verify` — Ed25519 signing & verification
  - `xbin keygen` — generate signing keypairs
  - `xbin trust` — manage trusted public keys
  - `xbin doctor` — system health checks (`--strict` for CI)
  - `xbin env` — show environment & build config
  - `xbin clean` — remove cache
  - `xbin completion <shell>` — shell completions (bash, zsh, fish, elvish, powershell)
  - `xbin man [dir]` — generate man pages
- **`.xbin.toml` config file** — defaults for all build flags
- **Shell completions** — `xbin completion bash/zsh/fish > file`
- **Man pages** — `xbin man /usr/local/share/man/man1/` (pre-generated in release tarballs)
- **10 integration tests** for Rust CLI
- **100% Rust core** — format, compression, detection, signing, assembly, tar, pkgmgr

### Changed
- Bumped version from 0.2.9 → 0.3.0
- xbin-stub cross-compile target: `aarch64-unknown-linux-musl` → `aarch64-unknown-linux-gnu`
- xbin doctor non-fatal by default, use `--strict` for CI
- Release workflow `needs` reduced to 3/4 jobs (skip slow macos-x64)
- find_binary now searches workspace-level `target/` directory

### Fixed
- find_binary now searches workspace-level `target/` directory
- Python CLI `--version` reads from pyproject.toml (was hardcoded "0.1.0")
- Version centralization: single source of truth in pyproject.toml
- Shell completion: bash/zsh/fish scripts generated at build time

## [0.2.9] - 2026-07-21
### Fixed
- xbin-stub and xbin-crypto marked optional in doctor checks

## [0.2.8] - 2026-07-21
### Added
- Python CLI shell completion (bash, zsh, fish)
- Python CLI `--strict` flag for doctor
- find_binary searches workspace-level target/ directory

## [0.2.7] - 2026-07-21
### Added
- Multi-arch GitHub releases (Linux x64/arm64, macOS x64/arm64)
- Release workflow triggered on GitHub release publish
- SSH signing for all commits/tags
- `xbin doctor` health check command
- `xbin env` environment info command
