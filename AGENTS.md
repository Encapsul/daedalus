# AGENTS.md — Instructions for coding agents

## Project overview

x.bin packages any app into a single self-extracting ELF binary. Rust workspace with 3 crates: `xbin-core` (library), `xbin-cli` (CLI), `xbin-stub` (launcher). Legacy Python CLI in `cli/` is being replaced by the Rust CLI.

## Build, lint, test commands

### Rust

```bash
# Build
cargo build --release --target x86_64-unknown-linux-musl -p xbin-stub
cargo build -p xbin-cli

# Lint (MUST pass before any commit)
cargo fmt --check
cargo clippy -p xbin-core --all-targets -- -D warnings
cargo clippy -p xbin-stub -- -D warnings
cargo clippy -p xbin-cli -- -D warnings

# Tests
cargo test --workspace

# Format
cargo fmt
```

### Python (legacy CLI in `cli/`)

```bash
cd cli
ruff check xbin/
black --check xbin/
black xbin/              # auto-format
ruff check --fix xbin/   # auto-fix
pytest                    # tests
```

## Project structure

```
x.bin/
├── xbin-core/           # Shared library: format, compress, detect, encrypt, layers, tar
│   ├── src/
│   │   ├── lib.rs       # Module index
│   │   ├── format.rs    # .xbin binary format v2-v5, footer, magic constants
│   │   ├── assembly.rs  # .xbin file assembly (stub + payload + meta + footer)
│   │   ├── compress.rs  # Zstd compression/decompression
│   │   ├── detect.rs    # Runtime detection (Python, Node, Deno, Java, etc.)
│   │   ├── encrypt.rs   # AES-256-GCM + HKDF key derivation
│   │   ├── layers.rs    # rootfs layer construction, /etc setup, filtered copy
│   │   ├── tar.rs       # Deterministic tar creation
│   │   ├── pkgmgr.rs    # Package manager detection (uv/poetry/pip, npm/pnpm/yarn)
│   │   ├── treeshake.rs # JS/Node tree-shaking (import/require analysis)
│   │   ├── minify.rs    # CSS minification, JS minification via terser
│   │   ├── dotenv.rs    # .env loading + secret detection
│   │   ├── otel.rs      # OpenTelemetry env setup
│   │   ├── persistent.rs# Persistent config (XDG dirs)
│   │   └── cron.rs      # Cron job scheduling
│   ├── Cargo.toml
│   └── pyproject.toml   # Python bindings (optional, feature-gated)
├── xbin-cli/            # Rust CLI (clap-based, replaces Python CLI)
│   ├── src/
│   │   ├── main.rs      # Entry point, subcommands
│   │   └── commands/    # build, inspect, scan, sign, verify, keygen, etc.
│   ├── Cargo.toml
│   └── tests/           # Integration tests (assert_cmd)
├── stub/                # Self-extracting launcher (Linux ELF)
│   ├── src/
│   │   ├── main.rs      # pivot_root, seccomp BPF, namespace isolation, exec
│   │   └── squashfs_extract.rs
│   └── Cargo.toml
├── cli/                 # Legacy Python CLI (being replaced)
├── .github/workflows/   # CI (preflight, clippy, build test) + release (multi-arch)
├── Makefile             # Build shortcuts
└── Cargo.toml           # Workspace root
```

## Code style

### Rust

- Edition 2021, `cargo fmt` is authoritative
- `xbin-core` uses pedantic clippy with many allows (see `Cargo.toml [lints.clippy]`)
- Prefer `Result::ok()` over `|e| e.ok()` (clippy redundant-closure-for-method-calls)
- Prefer `r"..."` over `r#"..."#` when no `#` in string (clippy needless-raw-string-hashes)
- Use `'\n'` not `"\n"` for single-char pattern matching (clippy single-char-pattern)
- Use `.contains_key()` over `.get().is_none()` (clippy unnecessary-get-then-check)
- Use `if let Some(v)` over `match` with `None => {}` (clippy single-match)
- Functions with >7 params: consider a config struct (clippy too-many-arguments)
- Release profile: `opt-level = "z"`, LTO, strip, `panic = "abort"` — optimized for tiny binaries

### Python (legacy CLI)

- ruff + black, target Python 3.12, line-length 88
- ruff rules: E, W, F, I, UP, B, SIM, RUF (ignore E501)

## Testing

- Rust unit tests are in each module under `#[cfg(test)] mod tests`
- Integration tests in `xbin-cli/tests/` use `assert_cmd`
- Run `cargo test --workspace` for all tests
- Python tests: `pytest` in `cli/`
- CI runs: preflight, clippy, build test, Python lint

## Git conventions

- Branches: `feat/*`, `fix/*`, `dev`, `main`
- Commits: signed (`git commit -S`), conventional format (`feat:`, `fix:`, `chore:`)
- PRs: pass clippy + fmt + tests before merge

## Boundaries

**Always do:**
- Run `cargo fmt` and `cargo clippy -- -D warnings` before committing
- Run `cargo test --workspace` to verify no regressions
- Preserve the `.xbin` footer format (magic `XBIN\x01`, footer magic `0xBEEF_CAFE`)

**Never do:**
- Commit secrets, keys, or `.env` files
- Change the `.xbin` binary format without updating `format.rs` version constants
- Remove clippy allows from `Cargo.toml` without understanding why they were added
- Use `unsafe` in `xbin-core` (only allowed in `stub/src/main.rs` for syscalls)

**Ask first:**
- Modifying the stub launcher (`stub/src/main.rs`) — security-critical code
- Changing encryption/signing logic in `encrypt.rs`
- Adding new runtime detection in `detect.rs`
- Modifying the release workflow in `.github/workflows/release.yml`
