# AGENTS.md — Instructions for coding agents

## Project overview

x.bin packages any app into a single self-extracting ELF binary. Rust workspace with 3 crates: `xbin-core` (library), `xbin-cli` (CLI), `xbin-stub` (launcher). Legacy Python CLI in `cli/` is being replaced by the Rust CLI.

## Build, lint, test commands

**Environment**: tools installed in `~/.local/bin`. Always prefix with:
```bash
export PATH="$HOME/.local/bin:$PATH"
```

### Rust

```bash
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
pytest                    # tests
```

## Code style

### Rust

- Edition 2021, `cargo fmt` is authoritative
- Release profile: `opt-level = "z"`, LTO, strip, `panic = "abort"` — tiny binaries
- Clippy pedantic with many allows (see `xbin-core/Cargo.toml [lints.clippy]`)
- Prefer `Result::ok()` over `|e| e.ok()`
- Prefer `r"..."` over `r#"..."#` when no `#` in string
- Use `'\n'` not `"\n"` for single-char pattern matching
- Use `.contains_key()` over `.get().is_none()`
- Use `if let Some(v)` over `match` with `None => {}`
- Functions with >7 params: use a config struct

## Security best practices (ANSSI-Rust)

Based on [ANSSI Secure Rust Guidelines](https://anssi-fr.github.io/rust-guide). These are **rules** (MUST) not suggestions.

- **DENV-STABLE**: Use stable toolchain only. Never nightly/beta.
- **DENV-CARGO-LOCK**: `Cargo.lock` MUST be tracked in version control.
- **LANG-UNSAFE**: No `unsafe` blocks in `xbin-core`. Only in `stub/src/main.rs`.
- **UNSAFE-NOUB**: Zero Undefined Behavior. No exceptions.
- **LANG-LIMIT-PANIC**: No `panic!()` in library code. Prefer `Result<T, E>`.
- **LANG-LIMIT-PANIC-SRC**: No `unwrap()`/`expect()` in `xbin-core` without context.
- **LANG-ARITH**: Use checked/wrapping/saturating arithmetic where overflow is possible.
- **MEM-NO-LEAK**: No `mem::forget` or `.leak()`.
- **FFI-SAFEWRAPPING**: All FFI calls MUST have safe wrappers.
- **LIBS-AUDIT**: Run `cargo audit` periodically.

## Testing

- Unit tests: `#[cfg(test)] mod tests` in each module
- Integration tests: `xbin-cli/tests/` use `assert_cmd`
- `cargo test --workspace` for all tests
- Python: `pytest` in `cli/`

## Git conventions

- Branches: `feat/*`, `fix/*`, `dev`, `main`
- Commits: signed (`git commit -S`), conventional format (`feat:`, `fix:`, `chore:`)
- PRs: pass clippy + fmt + tests before merge

## CLI design (clig.dev)

- Human-first: stdout = data, stderr = logs/errors
- Standard flags: `-h`/`--help`, `--version`, `-v`/`--verbose`, `-q`/`--quiet`, `-o`/`--output`, `--dry-run`, `--json`
- Exit codes: 0 = success, non-zero = failure
- No prompts in CI (require `--force` instead)

## Boundaries

**Always do:**
- Run `cargo fmt` and `cargo clippy -- -D warnings` before committing
- Run `cargo test --workspace` to verify no regressions
- Preserve the `.xbin` footer format (magic `XBIN\x01`, footer magic `0xBEEF_CAFE`)
- Verify any auto-fix from clippy/cargo-fix manually (ANSSI DENV-AUTOFIX)

**Never do:**
- Commit secrets, keys, or `.env` files
- Change the `.xbin` binary format without updating `format.rs` version constants
- Remove clippy allows from `Cargo.toml` without understanding why
- Use `unsafe` in `xbin-core` (only allowed in `stub/src/main.rs`)
- Override `debug-assertions` or `overflow-checks` in profiles
- Panic in library code — use `Result` (ANSSI LANG-LIMIT-PANIC)
- Leak memory via `mem::forget` or `.leak()` (ANSSI MEM-NO-LEAK)

**Ask first:**
- Modifying the stub launcher (`stub/src/main.rs`) — security-critical
- Changing encryption/signing logic in `encrypt.rs`
- Adding new `unsafe` blocks anywhere
- Adding new FFI bindings
