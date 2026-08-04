# Changelog

All notable changes to x.bin will be documented in this file.

Format based on [Keep a Changelog](https://keepachangelog.com/).

## [0.4.0] - 2026-07-29

### Added
- Auto-download PHP extensions from `shivammathur/php-builder` GitHub releases
- PHP extension detection from `composer.json` (`ext-*` requirements)
- Shared library bundling for downloaded PHP binaries (libssl, libgd, etc.)
- Standardized release asset naming: `xbin-<component>-<version>-<arch>-<os>.<ext>`
- `make dist` target for multi-arch release builds (x86_64, aarch64)
- `make release` target for GitHub release creation

### Fixed
- Ubuntu detection in `detect_linux_distro()` — Ubuntu's `/etc/os-release` contains
  `ID_LIKE=debian`, which caused downloading the wrong PHP build
- Missing shared libraries for downloaded PHP binaries at runtime
- Dead code: removed unused `download_php_extension()` (replaced by binary download)

## [0.3.2] - 2026-07-24

### Fixed
- ANSSI-Rust security fixes: deduplicated FFI wrappers, shared helpers, audit fixes
- Documentation updates: corrected stale Python CLI references, version badges, unsafe rules

## [0.3.1] - 2026-07-24

### Added
- Cron jobs (`--cron` flag)
- OpenTelemetry integration (`--otel` flag)
- Tree-shaking (`--treeshake` flag)
- HTML/JS/CSS minification (`--minify` flag)
- Custom health check port (`--health-port` flag)
- Persistent storage (`--persist` flag)
- Extra file inclusion (`--include` flag)
- Supervisor mode for multi-service apps

### Changed
- Single-service exec uses `execvp` (PATH lookup) instead of `execve`

## [0.3.0] - 2026-07-23

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
  - Full Unix man(7) sections: EXIT STATUS, ENVIRONMENT, FILES, SEE ALSO, AUTHORS, HISTORY, BUGS
- **`--env KEY=VALUE`** — repeatable flag to bake environment variables into the binary
- **`--env-file`** — load KEY=VALUE pairs from a file (now actually works)
- **10 integration tests** for Rust CLI
- **100% Rust core** — format, compression, detection, signing, assembly, tar, pkgmgr
- **Benchmark script** — `benchmarks/run-bench.sh` for measuring build performance

### Changed
- Bumped version from 0.2.9 → 0.3.0
- xbin-stub cross-compile target: `aarch64-unknown-linux-musl` → `aarch64-unknown-linux-gnu`
- xbin doctor non-fatal by default, use `--strict` for CI
- Release workflow rewritten to match toboggan working pattern
- find_binary now searches workspace-level `target/` directory
- Single-service exec uses `execvp` (PATH lookup) instead of `execve`
- Node.js entrypoint detection: checks package.json "main", "scripts.start", then fallback list

### Fixed
- **Sign command wrote corrupt V3 footer** — footer was written at the same offset as sig_block, overwriting it; now writes [sig_offset:u64le][core:84] after sig_block with proper set_len
- **Sign command opened file read-only** — changed `File::open` to `OpenOptions::new().read(true).write(true)`
- **Verify panicked on sig_size mismatch** — now validates sig_size==64 and uses fixed slice
- **`--env-file` resolved but never loaded** — the path was resolved but the file was never read; now parses KEY=VALUE lines
- **SHA-256 integrity check** — stub now always verifies `SHA-256(payload || meta_bytes)`, matching what the builder computes
- **Long tar paths** — removed manual `set_path()` on GNU header (100-byte limit) for npm deep deps
- **Entrypoint resolution** — generated argv is now runtime-aware (Python: `["python3", "/app/app.py"]`, Node: `["node", "/app/index.js"]`)
- **`cwd=/app`** set in metadata so stub `chdir`s before exec
- **`node_modules` included with `--no-install`** — copy filter respects the flag
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
