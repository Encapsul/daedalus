# AGENTS.md

## Project

x.bin packages any app into a single self-extracting ELF binary. Rust workspace (3 crates): `xbin-core` (library), `xbin-cli` (CLI, cross-platform), `stub` (launcher, Linux-only).

## Critical gotchas

- **vfat filesystem**: repo lives on vfat (no exec bit). Cargo target dir is `/tmp/xbin-stub-target` (set in `.cargo/config.toml`). Build artifacts cannot live in the repo tree.
- **PATH**: tools installed in `~/.local/bin`. Prefix with `export PATH="$HOME/.local/bin:$PATH"` when running pip-installed tools.
- **musl target**: stub builds with `--target $(uname -m)-unknown-linux-musl` for static linking. Requires `rustup target add` and a C compiler (musl-tools on Ubuntu).
- **CI runs clippy per-crate**, not workspace-wide: `cargo clippy -p xbin-core --all-targets -- -D warnings`, then same for `xbin-stub`, then `xbin-cli`.

## Commands

### Rust (primary)

```bash
cargo fmt --check                          # format check
cargo clippy --all-targets -- -D warnings  # lint (MUST pass before commit)
cargo test --workspace                     # all tests
cargo build --release                      # release build
cargo audit                                # dependency vulnerabilities (run in CI)
```

### Python (legacy CLI in `cli/`, deprecated)

This directory has been removed. The Rust CLI (`xbin-cli`) is now the only CLI.

```bash
# No Python CLI to lint/test — removed
```

### Verification loop (MANDATORY before finishing any change)

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

## Architecture

### Binary format (`xbin-core/src/format.rs`)

Layout: `[stub][payload][metadata][footer]`
- Footer magic: `0xBEEF_CAFE`, format magic: `XBIN\x01`
- Integrity hash: `SHA-256(payload || meta_bytes)` — computed at build, verified at runtime
- Format versions: v2 (plain), v3 (signed), v4 (encrypted), v5 (squashfs)

### Stub launcher (`stub/src/main.rs`)

Reads footer+metadata from `/proc/self/exe` → cache check → SHA-256 verify → extract (zstd+tar or squashfs) to `~/.cache/xbin/<hash>/rootfs/` → `execvp` entrypoint.

Entrypoint resolution in `detect.rs:resolve_entrypoint()`:
- Python: `["python3", "/app/app.py"]` (interpreter bare on PATH, app path absolute)
- Node: `["node", "/app/index.js"]`
- Go/Binary: `["/app/app"]`

### Unsafe boundary

- **`xbin-core` and `xbin-cli`**: zero `unsafe`. Memory safety via Rust type system.
- **`stub/src/main.rs`**: the only crate with `unsafe`. All `unsafe` blocks MUST have `SAFETY` comments. No `unsafe` outside FFI calls and `static mut`.

## Code style (high-signal)

- Edition 2021, `cargo fmt` is authoritative. `max_width = 100` in `stub/rustfmt.toml`.
- Release profile: `opt-level = "z"`, LTO, strip, `panic = "abort"` — tiny binaries.
- Clippy pedantic subset — do NOT add new `#[allow]` without a comment. See `xbin-core/Cargo.toml [lints.clippy]`.
- Rust functions: ≤ 30 lines. Python functions: ≤ 40 lines.
- Functions with >7 params: use a config struct.
- Prefer `Result::ok()` over `|e| e.ok()`. Prefer `if let Some(v)` over `match` with `None => {}`.
- Comments explain WHY, never WHAT.

## Security rules (ANSSI-Rust, MUST follow)

- DENV-STABLE: stable toolchain only, never nightly/beta.
- No `panic!()` in library code. Prefer `Result<T, E>`.
- No `unwrap()`/`expect()` in `xbin-core` without context.
- Use checked/wrapping/saturating arithmetic where overflow is possible.
- No `mem::forget` or `.leak()` (memory leak).
- All FFI calls MUST have safe wrappers.
- Ed25519 keys must have the Ed25519 bit set (CVE-2023-48022).
- No hardcoded secrets anywhere.

## Boundaries

**Always do:**
- Rebuild after every code change: `cargo build --release` before testing xbin on an app.
- Run verification loop before committing.
- Preserve the `.xbin` footer format (magic constants in `format.rs`).
- Verify any auto-fix from `cargo clippy --fix` manually (ANSSI DENV-AUTOFIX).

**Never do:**
- Commit secrets, keys, or `.env` files.
- Change the `.xbin` binary format without updating `format.rs` version constants.
- Remove clippy allows from `Cargo.toml` without understanding why.
- Use `unsafe` in `xbin-core` or `xbin-cli`.
- Override `debug-assertions` or `overflow-checks` in profiles.
- Panic in library code or leak memory.

**Ask first:**
- Modifying `stub/src/main.rs` — security-critical launcher.
- Changing encryption/signing logic in `encrypt.rs`.
- Adding new `unsafe` blocks or FFI bindings.

## Testing

- Unit tests: `#[cfg(test)] mod tests` in each module.
- Integration tests: `xbin-cli/tests/` use `assert_cmd`.
- `cargo test --workspace` for all Rust tests.
- `xbin-cli` depends on `reqwest` (blocking, `rustls-tls` feature) — no OpenSSL dependency.

## Git conventions

- Branches: `feat/*`, `fix/*`, `dev`, `main`.
- Commits: signed (`git commit -S`), conventional format (`feat:`, `fix:`, `chore:`).
- PRs: must pass clippy + fmt + tests before merge.

## Other instruction files

- `CLAUDE.md` — Claude Code specific guidance (agents/commands/skills pattern).
- `CODE_STYLE.md` — detailed style rules with rationale.
- `RULES.md` — ANSSI-Rust rules (also in `.cursor/rules/` format).
- `HANDOFF.md` — project status, known build constraints, performance data.
- `.opencode/` — agents, skills, and commands for OpenCode sessions.
