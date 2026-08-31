# PROJECT.md — daedalus project reference

## Project

daedalus packages any app into a single self-extracting binary. Rust workspace (3 crates): `daedalus-core` (library), `daedalus-cli` (CLI, cross-platform), `daedalus-stub` (launcher, Linux-only).

## Critical gotchas

- **vfat filesystem**: repo lives on vfat (no exec bit). Cargo target dir is `/tmp/daedalus-stub-target` (set in `.cargo/config.toml`). Build artifacts cannot live in the repo tree.
- **PATH**: tools installed in `~/.local/bin`. Prefix with `export PATH="$HOME/.local/bin:$PATH"` when running pip-installed tools.
- **musl target**: stub builds with `--target $(uname -m)-unknown-linux-musl` for static linking. Requires `rustup target add` and a C compiler (musl-tools on Ubuntu).
- **CI runs clippy per-crate**, not workspace-wide: `cargo clippy -p daedalus-core --all-targets -- -D warnings`, then same for `daedalus-stub`, then `daedalus-cli`.

## Commands

```bash
cargo fmt --check                          # format check
cargo clippy --all-targets -- -D warnings  # lint (MUST pass before commit)
cargo test --workspace                     # all tests
cargo build --release                      # release build
cargo audit                                # dependency vulnerabilities (run in CI)
```

## Verification loop (MANDATORY)

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

## Architecture

### Binary format (`daedalus-core/src/format.rs`)

Layout: `[stub][payload][metadata][footer]`
- Footer magic: `0xBEEF_CAFE`, format magic: `ERE\x01`
- Integrity hash: `SHA-256(payload || meta_bytes)` — computed at build, verified at runtime
- Format versions: v2 (plain), v3 (signed), v4 (encrypted), v5 (squashfs)

### Stub launcher (`stub/src/main.rs`)

Reads footer+metadata from `/proc/self/exe` → cache check → SHA-256 verify → extract (zstd+tar or squashfs) to `~/.cache/daedalus/<hash>/rootfs/` → `execvp` entrypoint.

Entrypoint resolution in `detect.rs:resolve_entrypoint()`:
- Python: `["python3", "/app/app.py"]` (interpreter bare on PATH, app path absolute)
- Node: `["node", "/app/index.js"]`
- Go/Binary: `["/app/app"]`

### Unsafe boundary

- **`daedalus-core` and `daedalus-cli`**: zero `unsafe`. Memory safety via Rust type system.
- **`stub/src/main.rs`**: the only crate with `unsafe`. All `unsafe` blocks MUST have `SAFETY` comments. No `unsafe` outside FFI calls and `static mut`.

## Code style (high-signal)

- Edition 2021, `cargo fmt` is authoritative. `max_width = 100` in `stub/rustfmt.toml`.
- Release profile: `opt-level = "z"`, LTO, strip, `panic = "abort"` — tiny binaries.
- Clippy pedantic subset — do NOT add new `#[allow]` without a comment. See `daedalus-core/Cargo.toml [lints.clippy]`.
- Rust functions: ≤ 30 lines. Python functions: ≤ 40 lines.
- Functions with >7 params: use a config struct.
- Prefer `Result::ok()` over `|e| e.ok()`. Prefer `if let Some(v)` over `match` with `None => {}`.
- Comments explain WHY, never WHAT.

## Security rules (ANSSI-Rust, MUST follow)

- DENV-STABLE: stable toolchain only, never nightly/beta.
- No `panic!()` in library code. Prefer `Result<T, E>`.
- No `unwrap()`/`expect()` in `daedalus-core` without context.
- Use checked/wrapping/saturating arithmetic where overflow is possible.
- No `mem::forget` or `.leak()` (memory leak).
- All FFI calls MUST have safe wrappers.
- Ed25519 keys must have the Ed25519 bit set (CVE-2023-48022).
- No hardcoded secrets anywhere.

## Boundaries

**Always do:**
- Rebuild after every code change: `cargo build --release` before testing daedalus on an app.
- Run verification loop before committing.
- Preserve the `.daedalus` footer format (magic constants in `format.rs`).
- Verify any auto-fix from `cargo clippy --fix` manually (ANSSI DENV-AUTOFIX).

**Never do:**
- Commit secrets, keys, or `.env` files.
- Change the `.daedalus` binary format without updating `format.rs` version constants.
- Remove clippy allows from `Cargo.toml` without understanding why.
- Use `unsafe` in `daedalus-core` or `daedalus-cli`.
- Override `debug-assertions` or `overflow-checks` in profiles.
- Panic in library code or leak memory.

**Ask first:**
- Modifying `stub/src/main.rs` — security-critical launcher.
- Changing encryption/signing logic in `encrypt.rs`.
- Adding new `unsafe` blocks or FFI bindings.

## Testing

- Unit tests: `#[cfg(test)] mod tests` in each module.
- Integration tests: `daedalus-cli/tests/` use `assert_cmd`.
- `cargo test --workspace` for all Rust tests.
- `daedalus-cli` depends on `reqwest` (blocking, `rustls-tls` feature) — no OpenSSL dependency.

## Git conventions

- Branches: `feat/*`, `fix/*`, `dev`, `main`.
- Commits: signed (`git commit -S`), conventional format (`feat:`, `fix:`, `chore:`).
- PRs: must pass clippy + fmt + tests before merge.

## Current state

- **Format**: v5 (SquashFS support)
- **Status**: Phase 1/2/3 COMPLETE — full Rust CLI, no Python dependency for builds
- **Build**: `cargo build --release` (or `make stub` for development)
- **CLI**: Rust CLI (`daedalus` binary). The legacy Rust CLI is the only CLI. Python CLI removed in v0.4.0.
- **Health check**: `daedalus doctor` or `make preflight`
- **Branches**: `main` (stable), `dev` (integration), `experimental-ft` (experimental features), `feat/*` / `fix/*` (features)
- **Release**: glow-style GitHub releases with per-platform archives. Tag `v*` triggers `.github/workflows/release.yml`.
- **Runtimes**: Python, Node.js, Deno, Java, Ruby, .NET/C#, Go, PHP, Perl, Hugo, Binary (11 total)
- **Framework support**: Next.js, Nuxt, Astro, Remix, SvelteKit, Express, Fastify, Hono, Django, FastAPI, Flask, Laravel, Symfony (auto-detected)
- **Rust core**: `daedalus-core` crate — format, compress, detect, pkgmgr, tar, assembly, sign, verify, scan, PyO3 bindings
- **Rust CLI**: `daedalus-cli` crate — 15 commands (build, run, inspect, scan, sign, verify, keygen, trust, doctor, env, clean, selftest, upgrade, completion, man)
- **Tests**: 127 Rust (106 daedalus-core + 17 daedalus-cli + 4 daedalus-stub) (0 failures)
- **Signing**: SSH Ed25519 (`~/.ssh/git_signing_key`), GitHub signing key id=1064819. Note: Codespaces `gh-gpgsign` proxy currently returns 403 (GPG signing not enabled) — recent commits are unsigned until the environment permits it.

## Release workflow

On every `v*` tag push, `.github/workflows/release.yml` builds one archive per platform, produces `checksums.txt`, then creates the GitHub release with all assets.

Naming convention (glow-style):
```
daedalus_<version>_<os>_<arch>.<ext>
```

Examples:
- `daedalus_0.6.0_linux_amd64.tar.gz` (daedalus + daedalus-stub + daedalus-crypto)
- `daedalus_0.6.0_linux_arm64.tar.gz`
- `daedalus_0.6.0_darwin_amd64.tar.gz`
- `daedalus_0.6.0_darwin_arm64.tar.gz`
- `daedalus_0.6.0_windows_amd64.tar.gz`
- `checksums.txt`

Platforms:
- Linux: amd64, arm64, i386, armv7, arm, riscv64, ppc64le (musl + gnu)
- Darwin: amd64, arm64
- Windows: amd64

## Build constraints

### PHP Apps

| Issue | Impact | Mitigation |
|-------|--------|------------|
| `composer` binary missing | Build fails immediately | Auto-install composer via downloader if absent |
| PHP extensions missing (ext-gd, ext-dom, ext-simplexml, ext-bcmath, ext-xml) | `composer install` exits 2 | Use `--ignore-platform-reqs` for portable builds |
| Static PHP builds lack extensions | Vendor deps can't be resolved | Embed system PHP with all extensions if available |
| No `vendor/` dir before install | site_packages empty | Run `composer install` and update `plan.site_packages` |
| Composer version mismatch | Lock file platform requirements fail | Pin composer version or use `--no-dev --ignore-platform-reqs` |

### Node.js Apps

| Issue | Impact | Mitigation |
|-------|--------|------------|
| `node` not on PATH (NVM shells) | Build fails at runtime detection | Check `~/.nvm/versions/node/*/bin/node` as fallback |
| `pnpm`/`yarn`/`bun` not installed | Lock file respected but manager missing | Auto-fallback to `npm install` |
| npm workspaces (`workspace:*`) | `npm install` exits 1 with EUNSUPPORTEDPROTOCOL | Detect workspace configs, use proper workspace-aware install |
| Network flakiness (ECONNRESET, ETIMEDOUT) | npm install fails mid-build | Auto-retry with backoff (3 attempts) |
| `package.json` present but PHP app | Node runtime wins over PHP | Heuristic: defer to PHP if `artisan`/`wp-config.php`/`symfony.lock` exists |

### General

| Issue | Impact | Mitigation |
|-------|--------|------------|
| Locale of subprocess output | Error messages in French/Chinese/etc | Added `--lang` flag to daedalus CLI |
| `vendor/` already in app layer | `shutil.copytree` fails with FileExistsError | `rmtree` before copy in `build_app_layer` |
| `node_modules` ignored in app layer copy | Dependencies not embedded | Update `plan.site_packages` after `install_deps` |
| Build on live USB (vfat) | No exec bit, no symlinks | Stub in `/tmp`, `std::fs::copy` not symlink |
| Network timeouts during fetch | Build fails | Retry with exponential backoff in `fetch_deps` and `install_deps` |

## Performance

Build for uptime-kuma (65MB output): **148s on Xeon w5-2465X 32 cores**.
On a laptop: **5-10 minutes**. On USB live (8GB RAM): even worse.

Optimizations applied:
- zstd level 19 → 3 (~10x faster compression)
- Multithreading via `zstdmt` (~Nx on N-core machines)
- Streaming tar→zstd (50% less memory)

Expected build time after optimization:
- **15-25s** on Xeon 32 cores (was 148s)
- **30-60s** on typical laptop (was 5-10min)
- **<2min** on constrained hardware (Raspberry Pi, old laptop)

## Dependencies

**Rust crates**: pyo3 "0.29", sha2 "0.10", serde/serde_json "1", ruzstd "0.7", zstd "0.13", tar "0.4", ed25519-dalek "2", aes-gcm "0.10", hkdf "0.12", reqwest "0.12" (rustls-tls).

**Security advisories**:
- pyo3 < 0.29.0: 3 CVEs (HIGH/MEDIUM/LOW) — FIXED (upgraded to 0.29)
- tar < 0.4.45: RUSTSEC-2026-0067/0068 — we have 0.4.46, safe
- sha2 < 0.9.8: old CVE — we have 0.10.9, safe

## Signing policy (MANDATORY)

All commits and tags MUST be signed.

```bash
git config --global gpg.format ssh
git config --global user.signingkey ~/.ssh/git_signing_key.pub
git config --global commit.gpgsign true
git config --global tag.gpgsign true
git config --global gpg.ssh.allowedSignersFile ~/.ssh/allowed_signers
```

Key: `~/.ssh/git_signing_key` (ed25519)
GitHub key: id=1064819, title="Signing Key", type=signing

## Release policy

1. Bump version in all `Cargo.toml` files
2. Update `CHANGELOG.md`
3. Commit: `chore: bump version to X.Y.Z and update changelog`
4. Tag: `git tag vX.Y.Z && git push origin vX.Y.Z`
5. GitHub Actions workflow `.github/workflows/release.yml` triggers automatically
6. Workflow builds all platforms, creates GitHub release with assets

## External references

- [clig.dev](https://clig.dev) — CLI design conventions
- [POSIX.1-2017 Ch.12](https://pubs.opengroup.org/onlinepubs/9799919797/) — Shell & Utilities
- [ANSSI-Rust](https://anssi-fr.github.io/rust-guide/) — Rust security guidelines
- [Google Doc Style](https://developers.google.com/style) — Documentation style
- [12-Factor CLI](https://medium.com/@jdxcode/12-factor-cli-apps-dd3c227a0e46) — CLI best practices

## Other instruction files

- `CLAUDE.md` — Claude Code specific guidance (agents/commands/skills pattern).
- `CODE_STYLE.md` — detailed style rules with rationale.
- `RULES.md` — ANSSI-Rust rules (also in `.cursor/rules/` format).
- `ROADMAP.md` — complete roadmap of features to implement and limitations to address.
- `SECURITY.md` — security policy and best practices.
- `CONTRIBUTING.md` — contribution guidelines.
- `.opencode/` — agents, skills, and commands for OpenCode sessions.
