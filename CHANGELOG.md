# Changelog

All notable changes to daedalus will be documented in this file.

Format based on [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Added
- **Multi-service builds**: `--entrypoint name=cmd,arg,...` now serializes named services into the binary metadata and the stub supervisor runs them. `--service-port NAME=PORT` and `--service-timeout NAME=SECONDS` configure the readiness probe (`ready_port`/`ready_timeout`) each service is gated on before the app is considered up.
- `daedalus inspect` now lists multi-service definitions (`Services:` section, `service.<name>` lines in `--plain` mode)
- `ServiceSpec` in `daedalus-core::assembly` serializing `{name, cmd, env, ready_port, ready_timeout}` — the schema the stub supervisor deserializes

### Changed
- System config detection module `daedalus-core/src/system_info.rs` (`SystemConfig`, `detect()`, `compute_universal_slices()`) for resource-adaptive builds

### Fixed
- Stub multi-service supervisor on Unix now execs via `execvp` (PATH search) instead of `execve`, so bare interpreter names like `python3` resolve at runtime — multi-service binaries can actually launch their services
- `find_in_bin_paths`/`is_executable_path` made cross-platform (were Windows-only), so the Unix supervisor shares the same interpreter-resolution fallback as the Windows and single-service paths
- Removed unused `libc_execve` FFI declaration and `env_to_cstrings` helper
- `daedalus-core/src/lib.rs`: restored `pub mod paths;` (was accidentally replaced when adding `system_info`)

## [0.7.0] - 2026-09-03

### Changed
- Per-runtime install-tree embedding: `embed_runtime_dir()` copies filtered runtime directories (Python DLLs/stdlib, Node modules, Electron assets) instead of single binary only, enabling fully self-contained `.de` binaries across platforms
- `RuntimeProfile` with `only`/`exclude` filters per runtime (python, node, electron have filters; deno/hugo/wasmtime self-contained)
- `tools_dir_for()` + `target_suffix()` + `interpreter_bin()` centralized binary resolution in `deps.rs`
- `resolve_cross_interpreter()` extracted from `embed_interpreters` to reduce clippy `too_many_lines`
- `python_host_install_dir()` via `sys.prefix` on Windows; `None` on Linux/macOS (deps resolved by `ldd` + `embed_python_config`)
- `--embed-interpreter python` CLI flag + CI `smoke-test-windows` job updated

### Fixed
- `is_cross` logic in pipeline: `target.is_some_and(|t| parse_target(t).0 != host_arch)` (was comparing `target_arch` string incorrectly on host builds)
- Clippy `needless_return` removed from `#[cfg(not(unix))]` blocks in `embed.rs`
- README: removed "Two distinct segments" enterprise impact breakdown; de-duplicated AI model packaging section (H1→H2)

### Added
- `embed_runtime_dir()` in `daedalus-core/src/embed.rs` with `copy_runtime_filtered()` and `matches_pattern()`
- `RuntimeProfile::self_contained()` const fn for self-contained runtimes
- `find_python_in()` helper for Windows Python discovery
- `which("python")` fallback in `ensure_python` for Windows hosts

## [0.6.0] - 2026-08-31

### Changed
- **File extension**: renamed from `.daedalus` to `.de` across CLI, docs, and examples
- **Command rename**: `upgrade-binary` → `migrate` for clarity
- **Error messages**: improved to include problem + cause + fix (e.g. "app directory not found" instead of "failed to canonicalize app path")

### Added
- **AES-256-GCM payload encryption**: optional external-key encryption for `.de` binaries
  - `daedalus build --encrypt <keyfile>` encrypts the payload at build time with AES-256-GCM + HKDF
  - `daedalus run --decrypt-key <keyfile>` decrypts the payload at runtime
  - Encryption metadata (salt, nonce, tag offset, encrypted size) is stored in the JSON metadata block
  - The binary never contains the key; key rotation is supported by re-encrypting

### Fixed
- Extension consistency between HN pitch (`.de`) and CLI output (`.de`)
- **Multi-arch release pipeline**: glow-style GitHub releases with per-platform archives
  - Linux amd64/arm64/386/arm/riscv64/ppc64le (musl + gnu)
  - Darwin amd64/arm64
  - Windows amd64
  - Automated checksums.txt generation
- **Runtime detection & build robustness**
  - Python: detect projects with `.py` files at root (sqlmap case)
  - Node: fallback to `npm install` when no lockfile is present
  - Rust: gracefully skip library/workspace crates without `[[bin]]`
- **External tool download URLs**
  - Deno: correct asset naming (`<arch>-<triple>`)
  - Hugo: include version from GitHub API in asset name
- **Symlink handling**: preserve symlinks during directory copy to avoid infinite loops

## [0.6.2] - 2026-08-31

### Changed
- **Release pipeline**: glow-style GitHub releases with per-platform archives and checksums
- **Dependency updates**: upgrade lru to 0.16.4, ratatui to 0.30, and other security fixes

### Fixed
- **CI failures**: resolve clippy `must_use_candidate` warnings and stub `missing_docs` warnings
- **Release workflow**: ensure GitHub releases are created with changelog notes and all artifacts
- **Documentation**: audit and fix Unix man-page doc comments across daedalus-cli and daedalus-stub
- **Windows embedding**: skip `ldd` dependency resolution on non-Unix so the interpreter (e.g. official `node.exe`) is embedded self-contained instead of aborting the build
- **i386 stub build**: gate socket syscalls behind `SYS_socketcall` on 32-bit x86 so every architecture in the release matrix builds

## [0.6.1] - 2026-08-31

### Changed
- **Release pipeline**: glow-style GitHub releases with per-platform archives and checksums
- **Dependency updates**: upgrade lru to 0.16.4, ratatui to 0.30, and other security fixes

### Fixed
- **CI failures**: resolve clippy `must_use_candidate` warnings and stub `missing_docs` warnings
- **Release workflow**: ensure GitHub releases are created with changelog notes and all artifacts
- **Documentation**: audit and fix Unix man-page doc comments across daedalus-cli and daedalus-stub

## [0.6.0] - 2026-08-31

### Added
- **SISR self-update engine**: a `.daedalus` can update itself from signed deltas
  - `daedalus build --self-update` enables the engine; `daedalus upgrade-binary`
    migrates existing binaries
  - Incremental updates: manifests list changed chunks with Ed25519 signatures
    and a Merkle root; the launcher fetches, verifies, rebuilds and atomically
    swaps the binary in place
  - Health gate with automatic rollback: a crashing new version is quarantined
    and the previous one is re-executed
  - Update URL resolution: `--daedalus-update <URL>` > `$DAEDALUS_UPDATE_URL` >
    embedded `meta.update_url`
- Transparent v1 → v2 runtime migration for legacy binaries
- Cross-compilation support (`--target aarch64`, ...) with per-arch Node.js
  download and N-API addon embedding
- Landlock filesystem sandboxing (Linux, kernel ≥ 5.13)
- Laravel Octane (RoadRunner) detection and `rr` embedding
- Ruby native gem `.so` dependencies embedded via `ldd` scan
- PHP version constraint checking in `composer.json`
- pnpm `.pnpm` store excluded during copy (avoids massive duplication)
- `daedalus publish` command; pip retry/timeout for proxy resilience
- Clean remote cache design + single app-hash cache key
- E2E SISR tests, network fault injection, and property-based fuzzing

### Changed
- Release pipeline: multi-platform builds on native runners (linux amd64/arm64
  + extended musl/gnu targets, darwin amd64/arm64), `checksums.txt`, publish
  via `gh release`

## [0.4.0] - 2026-08-06

### Added
- Auto-download PHP extensions from `shivammathur/php-builder` GitHub releases
- PHP extension detection from `composer.json` (`ext-*` requirements)
- Shared library bundling for downloaded PHP binaries (libssl, libgd, etc.)
- `make dist` target for multi-arch release builds (x86_64, aarch64)
- `make release` target for GitHub release creation
- Node.js auto-download now caches in `~/.cache/daedalus/build-tools` (mode 0700)
  instead of a world-writable `/tmp` directory

### Changed
- Standardized release asset naming (glow-style):
  `daedalus_<version>_<os>_<arch>.<ext>` with `checksums.txt`
- Launcher hardened: all remaining `unwrap()`/`expect()` calls in the stub
  converted to `Result` handling — the launcher never panics on malicious input

### Fixed
- Ubuntu detection in `detect_linux_distro()` — Ubuntu's `/etc/os-release` contains
  `ID_LIKE=debian`, which caused downloading the wrong PHP build
- Missing shared libraries for downloaded PHP binaries at runtime
- Dead code: removed unused `download_php_extension()` (replaced by binary download)
- Isolation level 2 always failed with `ENOENT`:
  - Embedded runtimes shipped without their dynamic loader (`ld-linux.so`),
    so the kernel could not `exec` them after `pivot_root`. The ELF
    interpreter is now bundled into the rootfs.
  - `execvp` resolved relative `PATH`/`LD_LIBRARY_PATH` entries against the
    post-`pivot_root` cwd (`/app`). Absolute paths are used in the pivot branch.
  - `go`/`binary` runtimes were wrapped in `bash`, which was not bundled; they
    are now executed directly.
- `--isolation` now fails closed: unknown values are rejected instead of
  silently downgrading to level 1
- Signing keys are zeroized (`zeroize`) after use during `keygen` and `sign`

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
  - `daedalus build` — package any app into self-extracting ELF
  - `daedalus inspect` — read metadata from .daedalus
  - `daedalus scan` — find .daedalus files recursively
  - `daedalus sign` / `daedalus verify` — Ed25519 signing & verification
  - `daedalus keygen` — generate signing keypairs
  - `daedalus trust` — manage trusted public keys
  - `daedalus doctor` — system health checks (`--strict` for CI)
  - `daedalus env` — show environment & build config
  - `daedalus clean` — remove cache
  - `daedalus completion <shell>` — shell completions (bash, zsh, fish, elvish, powershell)
  - `daedalus man [dir]` — generate man pages
- **`.daedalus.toml` config file** — defaults for all build flags
- **Shell completions** — `daedalus completion bash/zsh/fish > file`
- **Man pages** — `daedalus man /usr/local/share/man/man1/` (pre-generated in release tarballs)
  - Full Unix man(7) sections: EXIT STATUS, ENVIRONMENT, FILES, SEE ALSO, AUTHORS, HISTORY, BUGS
- **`--env KEY=VALUE`** — repeatable flag to bake environment variables into the binary
- **`--env-file`** — load KEY=VALUE pairs from a file (now actually works)
- **10 integration tests** for Rust CLI
- **100% Rust core** — format, compression, detection, signing, assembly, tar, pkgmgr
- **Benchmark script** — `benchmarks/run-bench.sh` for measuring build performance

### Changed
- Bumped version from 0.2.9 → 0.3.0
- daedalus-stub cross-compile target: `aarch64-unknown-linux-musl` → `aarch64-unknown-linux-gnu`
- daedalus doctor non-fatal by default, use `--strict` for CI
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
- daedalus-stub and daedalus-crypto marked optional in doctor checks

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
- `daedalus doctor` health check command
- `daedalus env` environment info command
