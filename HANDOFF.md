# HANDOFF.md — x.bin project status

## Current state

- **Format**: v5 (SquashFS support)
- **Status**: Phase 1/2/3 COMPLETE — full Rust CLI, no Python dependency for builds
- **Build**: `cargo build --release` (or `make stub` for development)
- **CLI**: Rust CLI (`daedalus` binary). The legacy Python CLI (`cli/`) was removed.
- **Health check**: `daedalus doctor` or `make preflight`
- **Branches**: `main` (stable), `dev` (integration), `feat/*` / `fix/*` (features)
- **Release**: create release on GitHub UI → `on: release: types: [published]` triggers workflow → builds 4 platforms → uploads binaries + SHASUMS256.txt
- **Runtimes**: Python, Node.js, Deno, Java, Ruby, .NET/C#, Go, PHP, Perl, Binary, Hugo (11 total)
- **Framework support**: Next.js, Nuxt, Astro, Remix, SvelteKit, Express, Fastify, Hono, Django, FastAPI, Flask, Laravel, Symfony (auto-detected)
- **Rust core**: `daedalus-core` crate — format, compress, detect, pkgmgr, tar, assembly, sign, verify, scan, PyO3 bindings
- **Rust CLI**: `daedalus-cli` crate — 15 commands (build, run, inspect, scan, sign, verify, keygen, trust, doctor, env, clean, selftest, upgrade, completion, man)
- **Tests**: 127 Rust (106 daedalus-core + 17 daedalus-cli + 4 daedalus-stub) (0 failures)
- **Last updated**: 2026-08-04
- **Signing**: SSH Ed25519 (`~/.ssh/git_signing_key`), GitHub signing key id=1064819. Note: Codespaces `gh-gpgsign` proxy currently returns 403 (GPG signing not enabled) — recent commits are unsigned until the environment permits it.
- **Release workflow**: `on: release: types: [published]` — create release on GitHub UI → workflow builds 4 platforms → uploads tar.gz + SHASUMS256.txt

---

## Recent Commits (2026-07-24)

```
f8048fd benchmark: add build reports for 6 PHP apps (SuiteCRM, Filament, InvoiceNinja, OpenEMR, Roundcube, WooCommerce)
1a1c91e feat: improve PHP/Node app builds, add monorepo/workspace support, --lang flag
0bffc88 feat(cli): ungate progress messages, add --json, fix library debug output
e22be5e chore(core): remove dead code — unused fns, duplicates, layers module
f1e9f1e fix(cli): ANSSI hardening + clig.dev compliance
4ad8ad4 fix(core): ANSSI hardening + perf — error propagation, LazyLock, regex caching
fee6f8c fix(stub): ANSSI hardening — cstr error propagation, bounds checks, seccomp docs
```

Version bumped to **0.4.0** across all crates.

---

## Known Build Constraints (Discovered During Multi-App Packaging)

### PHP Apps

| Issue | Impact | Mitigation |
|-------|--------|------------|
| `composer` binary missing | Build fails immediately | Auto-install composer via downloader if absent |
| PHP extensions missing (ext-gd, ext-dom, ext-simplexml, ext-bcmath, ext-xml) | `composer install` exits 2 | Use `--ignore-platform-reqs` for portable builds |
| Static PHP builds lack extensions | Vendor deps can't be resolved | Embed system PHP with all extensions if available |
| No `vendor/` dir before install | site_packages empty | Run `composer install` and update `plan.site_packages` |
| Composer version mismatch | Lock file platform requirements fail | Pin composer version or use `--no-dev --ignore-platform-reqs` |

**Current apps tested:**
- SuiteCRM-hotfix: needs `ext-gd`, `ext-simplexml`, `ext-dom`
- InvoiceNinja 5: needs `ext-bcmath`, `ext-dom`, `ext-simplexml`, `ext-xml`
- OpenEMR: needs `ext-gd`, `ext-mbstring`, `ext-zip`, `ext-xml`
- Roundcube: needs `ext-gd`, `ext-dom`
- Filament: Laravel app, needs full LAMP stack extensions

### Node.js Apps

| Issue | Impact | Mitigation |
|-------|--------|------------|
| `node` not on PATH (NVM shells) | Build fails at runtime detection | Check `~/.nvm/versions/node/*/bin/node` as fallback |
| `pnpm`/`yarn`/`bun` not installed | Lock file respected but manager missing | Auto-fallback to `npm install` |
| npm workspaces (`workspace:*`) | `npm install` exits 1 with EUNSUPPORTEDPROTOCOL | Detect workspace configs, use proper workspace-aware install |
| Network flakiness (ECONNRESET, ETIMEDOUT) | npm install fails mid-build | Auto-retry with backoff (3 attempts) |
| `package.json` present but PHP app | Node runtime wins over PHP | Heuristic: defer to PHP if `artisan`/`wp-config.php`/`symfony.lock` exists |

**Current apps tested:**
- WooCommerce: pnpm workspace monorepo, `npm install` fails
- Filament: has `package.json` but is Laravel/PHP app

### General

| Issue | Impact | Mitigation |
|-------|--------|------------|
| Locale of subprocess output | Error messages in French/Chinese/etc | Added `--lang` flag to daedalus CLI |
| `vendor/` already in app layer | `shutil.copytree` fails with FileExistsError | `rmtree` before copy in `build_app_layer` |
| `node_modules` ignored in app layer copy | Dependencies not embedded | Update `plan.site_packages` after `install_deps` |
| Build on live USB (vfat) | No exec bit, no symlinks | Stub in `/tmp`, `std::fs::copy` not symlink |
| Network timeouts during fetch | Build fails | Retry with exponential backoff in `fetch_deps` and `install_deps` |

---

## Performance Optimization (Current)

### Problem

Build for uptime-kuma (65MB output): **148s on Xeon w5-2465X 32 cores**.
On a laptop: **5-10 minutes**. On USB live (8GB RAM): even worse.

Root causes identified:
1. **zstd level 19** — extremely slow. Level 3 is 10x faster for ~5% larger output
2. **Buffered tar** — entire uncompressed tar (300-500MB) buffered in memory before compression
3. **Single-threaded compression** — zstd not using available CPU cores
4. **No streaming** — tar→bytes→compress→bytes, doubling memory usage

### Solution Applied

| Optimization | Before | After | Impact |
|-------------|--------|-------|--------|
| zstd level | 19 | **3** | ~10x faster compression |
| Multithreading | None | **all cores** | ~Nx on N-core machines |
| Streaming tar→zstd | Buffered in memory | **Direct pipe** | 50% less memory |
| Default `DEFAULT_LEVEL` constant | Hardcoded 19 | **3** | All callers updated |

Expected build time after optimization:
- **15-25s** on Xeon 32 cores (was 148s)
- **30-60s** on typical laptop (was 5-10min)
- **<2min** on constrained hardware (Raspberry Pi, old laptop)

### What Changed in Code

**compress.rs**:
- `DEFAULT_LEVEL = 3` (was hardcoded 19)
- `compress()` uses level 3 + `multithread(num_cpus())`
- New `num_cpus()` helper using `available_parallelism()`

**tar.rs**:
- Refactored: shared `append_entries()` helper for all tar creation
- New `create_tar_zstd()` — streaming tar→zstd, never buffers full tar
- New `create_tar_streaming<W: Write>()` — generic streaming to any writer
- `create_deterministic_tar()` refactored to use shared helper

**build.rs**:
- Uses `create_tar_zstd()` instead of separate tar+compress steps
- Added timing output for compress phase (verbose mode)
- Removed hardcoded level 19

**Cargo.toml**:
- `zstd` now uses `features = ["zstdmt"]` for multithreading

---

## Benchmark Data

Located in `benchmarks/`:

| File | Machine | Build Time | Output | Peak RSS |
|------|---------|-----------|--------|----------|
| `uptime-kuma-20260723-183413.md` | Xeon w5-2465X 32c, NVMe | 148s | 65.4MB | 660MB |
| `uptime-kuma-20260723-183002.md` | Same | 151.9s | 65.4MB | 660MB |

Machine specs (Xeon run):
- CPU: Intel Xeon w5-2465X, 32 cores
- RAM: 251.3 GB
- Disk: NVMe 959GB
- Disk I/O: Write 64MB = 410ms, Read 64MB = 15ms

### 8GB tmpfs live USB estimate

- Peak RSS: 660MB → **YES** (fits in 8GB)
- Peak RSS + tmpfs overhead (~2×): 1321MB → **YES**
- With streaming optimization: RSS drops to ~300-400MB (no full tar buffer)

---

## Dead Code Removed

| Item | File | Reason |
|------|------|--------|
| `compress_tar_zstd()` | compress.rs | Thin wrapper, never called |
| `decompress_zstd()` | compress.rs | Thin wrapper, never called |
| `PAYLOAD_FORMAT_ZSTD_TAR` | format.rs | Literal used directly |
| `Footer::footer_size()` | format.rs | Never called |
| `get_otel_config()` | otel.rs | Never wired into pipeline |
| `CRYPTO_NONE` | encrypt.rs | Duplicate of format constant |
| `CRYPTO_AES_256_GCM` | encrypt.rs | Duplicate of format constant |
| `layers` module | lib.rs + layers.rs | 4 pub fns, zero imports |

### Remaining Dead Code (Low Priority)

- `#[allow(dead_code)]` fields in stub: `Metadata::runtime`, `CryptoMeta::tag_offset`, `Layer::kind`, `Layer::uncompressed_size` — kept for JSON deserialization forward compatibility
- 6 `eprintln!` calls in daedalus-core (treeshake.rs, minify.rs, dotenv.rs) — behind `verbose` flag but library shouldn't emit to stderr. Requires refactoring function signatures to return messages. Not removed.
- 18 `pub` functions with zero external callers — mostly intentional library API. Internal helpers (treeshake, dotenv, minify) could be `pub(crate)` but harmless.

---

## Test Results

| Crate | Tests | Clippy |
|-------|-------|--------|
| daedalus-core | 106 passed | Clean |
| daedalus-stub | 4 passed | Clean |
| daedalus-cli | 17 passed (7 unit + 10 integration) | Clean |

**Note:** daedalus-cli uses `reqwest` with `rustls-tls` (no OpenSSL) — it builds and tests without `libssl-dev`.

---

## Best practices references

### CLI design
- **[clig.dev](https://clig.dev)** — Command Line Interface Guidelines (Aanand Prasad, Ben Firshman, Carl Tashian). Primary reference for our CLI UX.
- **[Better CLI](https://bettercli.org/)** — CLI Design Guide & Reference. Covers lifecycle, config, distribution, security, analytics.
- **[12 Factor CLI Apps](https://medium.com/@jdxcode/12-factor-cli-apps-dd3c227a0e46)** — Config via env vars, self-contained binaries, strict separation of build/release/run.
- **[GNU Coding Standards](https://www.gnu.org/prep/standards/html_node/Command_002dLine-Interfaces.html)** — POSIX conventions, `--help`, `--version`.

### CLI design — tool-specific inspiration
- **Docker CLI** — noun-verb pattern (`docker container create`), `--format` Go templates, shell completion for 4 shells, `config.json` for persistent config, `NO_COLOR` support.
- **Bun** — single binary, zero deps, fast startup, `bunfig.toml` config file, `--verbose` global flag.
- **Wasmer** — `wasmer.toml` package manifest, `wasmer run` with runtime detection, template system.

### Rust
- **[Command Line Applications in Rust](https://rust-cli.github.io/book/)** — Config files, exit codes, human/machine communication, progress bars, signal handling.
- **[Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)** — Naming, interoperability, macros, documentation, predictability, flexibility, type safety, dependability, debuggability, future-proofing.
- **[The Rustonomicon](https://doc.rust-lang.org/nomicon/)** — Unsafe Rust: FFI, memory model, type punning, uninitialized memory, concurrency. Only needed for low-level stub work.
- **[clap](https://docs.rs/clap)** — Derive-based arg parsing, shell completion generation, `#[command(flatten)]` for shared args.
- **[anyhow](https://docs.rs/anyhow)** — Error context with `.context("message")`, `bail!()` macro for early returns.
- **[human-panic](https://docs.rs/human-panic)** — User-friendly crash reports instead of ugly backtraces.

### What we follow from each reference

| Rule | Source | Status |
|------|--------|--------|
| stdout=machine, stderr=messaging | clig.dev | ✅ |
| `--json` for machine output | clig.dev | ✅ inspect, doctor |
| `--no-color`, `NO_COLOR`, `TERM=dumb` | clig.dev | ✅ |
| `-q`/`--quiet` | clig.dev | ✅ |
| Confirm before dangerous actions | clig.dev | ✅ clean --all, doctor --fix |
| `--force` / `-f` for scripts | clig.dev | ✅ |
| `--version` reads from pyproject.toml | clig.dev | ✅ (was hardcoded 0.1.0) |
| Shell completion (bash/zsh/fish) | Docker/Bun | ✅ `daedalus completion {bash,zsh,fish}` |
| `--strict` mode | Better CLI | ✅ doctor --strict |
| `human-panic` crash reports | Rust CLI Book | ✅ |
| `anyhow` for error context | Rust CLI Book | ✅ |
| Global `--verbose` flag | Docker/Bun | ✅ |
| Exit codes 0/1/2 | clig.dev + BSD sysexits | ✅ documented in help |
| `--dry-run` for destructive ops | clig.dev | ✅ daedalus build --dry-run |
| Config file (`.ere.toml`) | Docker/Bun/Wasmer | ✅ .ere.toml in app dir |
| Shell completion (Rust CLI) | clap_complete | ✅ `daedalus completion bash/zsh/fish` |
| Man pages | Rust CLI Book | ✅ `daedalus man [dir]` |
| `human-panic` crash reports | Rust CLI Book | ✅ |
| `anyhow` for error context | Rust CLI Book | ✅ |
| Global `--verbose` flag | Docker/Bun | ✅ |
| Exit codes 0/1/2 | clig.dev + BSD sysexits | ✅ documented in help |
| Man pages | Rust CLI Book | ✅ `daedalus man [dir]` |

## Config file (`.ere.toml`)

Place `.ere.toml` in your app directory. CLI flags override config file values.

```toml
[package]
version = "1.0.0"
author = "Your Name"
description = "My awesome app"
license = "MIT"

[build]
isolation = "sandbox"
seccomp = true
encrypt = false
squashfs = false
target = "x86_64"
no_install = false
env_file = ".env"
```

### Shell completion

```bash
# Bash
daedalus completion bash >> ~/.bashrc

# Zsh
daedalus completion zsh >> ~/.zshrc

# Fish
daedalus completion fish > ~/.config/fish/completions/daedalus.fish
```

### Man pages

```bash
daedalus man /usr/local/share/man/man1/
```

### Dry run

```bash
daedalus build ./myapp --dry-run --verbose
```

---

## Signing policy (MANDATORY)

**All commits and tags MUST be signed.** This is enforced by git config:

```bash
# Global config (already set):
git config --global gpg.format ssh
git config --global user.signingkey ~/.ssh/git_signing_key.pub
git config --global commit.gpgsign true
git config --global tag.gpgsign true
git config --global gpg.ssh.allowedSignersFile ~/.ssh/allowed_signers
```

**Key**: `~/.ssh/git_signing_key` (ed25519, email: teddams047@gmail.com)
**GitHub key**: id=1064819, title="Signing Key", type=signing (added via `gh ssh-key add --type signing`)

**New tags**: always use `git tag -s vX.Y.Z -m "message"` (NOT `git tag -a`).
**New commits**: signing is automatic (`commit.gpgsign=true`).

**Re-signing existing tags** (if needed):
```bash
for tag in $(git tag -l); do
  COMMIT=$(git rev-parse "$tag"^{commit})
  MSG=$(git log --format=%s -1 "$COMMIT")
  git tag -d "$tag"
  git tag -s "$tag" -m "$tag - $MSG" "$COMMIT"
done
git push --force --tags origin
```

---

## Release policy (MANDATORY)

**Workflow**: `.github/workflows/release.yml` triggers on `release: types: [published]` (toboggan pattern).

**How to create a release**:
1. Go to https://github.com/Tednoob17/daedalus/releases/new
2. **Tag**: create new tag `vX.Y.Z` (or select existing)
3. **Release title**: `x.bin vX.Y.Z` (or `x.bin vX.Y.Z — <codename>`)
4. **Description**: write changelog, install instructions, etc.
5. Click "Publish release"
6. Workflow auto-triggers: builds linux-x64, linux-arm64, macos-arm64, macos-x64 → uploads tar.gz + SHASUMS256.txt

**Package names**: `daedalus-{os}-{arch}.tar.gz` (e.g., `daedalus-linux-x64.tar.gz`)
**Each tar.gz contains**: `bin/daedalus-stub`, `bin/daedalus-crypto`, `bin/daedalus` (wrapper), `lib/python/daedalus/` (CLI)

---

## Dependency maintenance policy

**Rust crates** (`daedalus-core/Cargo.toml`, `stub/Cargo.toml`):
- Run `cargo update` periodically to get latest semver-compatible versions
- Check https://crates.io for major version bumps (sha2, ruzstd, etc.)
- Dependabot alerts: monitor and fix promptly
- Current deps: pyo3 "0.29", sha2 "0.10", serde/serde_json "1", ruzstd "0.7", zstd "0.13", tar "0.4"

**Python packages** (`cli/pyproject.toml`):
- Removed — the Python CLI (`cli/`) no longer exists.

**Security advisories**:
- pyo3 < 0.29.0: 3 CVEs (HIGH/MEDIUM/LOW) — FIXED (upgraded to 0.29)
- tar < 0.4.45: RUSTSEC-2026-0067/0068 — we have 0.4.46, safe
- sha2 < 0.9.8: old CVE — we have 0.10.9, safe

---

## LLM Verification Loop (MANDATORY)

**Every time you modify code, before finishing your turn, run this checklist:**

### ⚠️ RULE: NEVER commit without running ALL of these first:
```bash
cd cli && python3 -m ruff check daedalus/     # lint
cd cli && python3 -m black --check daedalus/  # format
cd cli && python3 -m pytest tests/ -q     # tests
```

### 1. Did the code change?
If you edited any `.py`, `.rs`, `.toml`, or `.yml` file → continue. Otherwise skip.

### 2. Does it compile / import?
```bash
cargo check                                          # Rust core & CLI
cargo clippy -p daedalus-core --all-targets -- -D warnings  # Lint
cargo fmt --check                                    # Format
cargo test --workspace                               # Tests
```
**ALL FOUR MUST PASS before finishing your turn. No exceptions.**

### 3. Does the app work?
```bash
cargo build -p daedalus-cli && ./target/release/daedalus build examples/hello-web -o /tmp/test.ere 2>&1
```
If this fails, your change broke the build. Fix it before proceeding.

### 4. Is HANDOFF.md up to date?
If you added a feature, changed the format, or modified the architecture:
- Update the relevant section above
- Update the "Current state" header
- Add a "Next steps" item if relevant

### 5. Is README.md up to date?
If you added a CLI command, changed the install process, or modified the
format: update README.md (quickstart, quick links, "How it works" diagram).

### 6. Is CODE_STYLE.md up to date?
If you changed the linting or formatting config, update CODE_STYLE.md.

### 7. Are docs up to date?
If you changed the builder, launcher, format, or isolation: update the
relevant `docs/src/` page.

### 8. No regressions?
Run the full test sequence:
```bash
daedalus build examples/hello-web -o /tmp/t.ere
daedalus inspect /tmp/t.ere
daedalus keygen --key-dir /tmp/tk
daedalus sign /tmp/t.ere --key /tmp/tk/*.key
daedalus verify /tmp/t.ere --trusted-dir /tmp/tk
```
All must pass. If any fails, your change introduced a regression.

### Rule: never commit without passing this loop.

## Cross-compilation (`--target aarch64`) — 2026-07-18

- **File**: `cli/daedalus/cross.py`
- `download_vendored_python(runtime, arch)`: downloads prebuilt Python/Node from `python-build-standalone` (astral-sh) or Node.js official, extracts to `~/.cache/daedalus/vendor/{runtime}-{arch}/`
- `pip_download_target(app_dir, requirements, venv_dir, target_arch)`: runs `pip download --only-binary=:all: --platform {manylinux tag} --python-version {ver}` to fetch wheels for target arch
- `_vendored_python_version(vendored_root)`: detects Python version from vendored `lib/pythonX.Y/` directory
- `_unpack_wheel(wheel_path, site_packages_dir)`: extracts wheel `.zip` into site-packages for cross-build
- **File**: `cli/daedalus/build.py`
- `build()` rejects non-Python runtimes for cross-build (`node`/`deno` → clear error message)
- `_pip_install_requirements()` accepts `target_arch`: uses `pip_download_target()` when cross-building, falls back to normal pip when native
- `_build_runtime_layer()` / `_build_layers()` / `_build_layers_squashfs()` pass `target_arch` through for cross-build pip
- `_build_runtime_layer()` skips `.so` resolution when using vendored cross-python (no host libs to resolve)

## Dependency checks — 2026-07-18

- **File**: `cli/daedalus/doctor.py`
- `daedalus doctor` subcommand: checks Python, pip, cargo, rustc, musl target, C compiler, zstd, mksquashfs, node, deno, cryptography, ruff, black, daedalus-stub, daedalus-crypto
- Each check returns (ok, detail). Required vs optional. Returns exit 1 if any required check fails.
- **File**: `Makefile`
- `make preflight`: quick prerequisite check (python3, pip, cargo, rustc, musl target, cc, zstd). Exit 1 on failure.
- **File**: `cli/pyproject.toml`
- Added `[project.optional-dependencies]`: `encrypt` (cryptography), `python310` (tomli), `dev` (ruff, black), `all`
- **File**: `stub/Cargo.toml`
- Fixed misleading comment: `backhand` (squashfs) requires C compiler via `zstd-sys` — project is not purely pure-Rust

## Doctor --fix (auto-install missing deps) — 2026-07-20

- **File**: `cli/daedalus/doctor.py` — `--fix` flag, `--force` / `-f` flag
- `daedalus doctor --fix`: attempts to auto-install missing required prerequisites
- `daedalus doctor --fix --force`: skips confirmation prompt (for scripts/CI)
- **Fixable checks**: musl target (`rustup target add`), zstd (`apt install`), mksquashfs (`apt install`), cryptography/ruff/black (`pip install`), daedalus-stub/daedalus-crypto (`make stub`)
- **Non-fixable checks** (manual install required): Python version, pip, cargo, rustc, C compiler, node, deno
- **Safety**: confirms interactively before fixing (unless `--force`); each fix has a timeout; re-verifies after fix; continues on individual failures
- **clig.dev compliance**: `--fix` follows "confirm before dangerous actions" guideline; `--force` for scriptability; exit 0 on full success, 1 on partial/full failure

## Incremental update (`--update` flag) — 2026-07-20

- **File**: `cli/daedalus/build.py` — `--update` flag on build subcommand
- **File**: `cli/daedalus/assembly.py` — `app_hash` and `rt_deps_hash` params on `build_meta_json()`
- **How it works**:
  - First build: computes `app_hash` (SHA-256 of all app files) and `rt_deps_hash` (SHA-256 of requirements.txt), stores them in the .ere metadata JSON
  - `daedalus build --update`: reads existing .ere, compares hashes:
    - Same app + same runtime deps → early return ("everything up to date")
    - Same runtime deps, app changed → reuses existing runtime squashfs/zstd blob, only rebuilds app layer
    - Runtime deps changed → full rebuild
- **Helper functions**:
  - `_hash_app_files(app_dir)`: SHA-256 of all files in app_dir (excluding .venv, node_modules, .git, etc.)
  - `_read_existing_daedalus(daedalus_path, verbose)`: reads footer → meta JSON → extracts runtime layer blob
- **Benefits**: 2-5x faster rebuilds when only app code changes (runtime layer is 12+ MB, app layer is small)
- **Tested**: build → modify app.py → build --update → runtime layer SHA unchanged, app layer SHA changed ✓

## daedalus scan (discover .ere files) — 2026-07-20

- **File**: `cli/daedalus/scan.py` — `scan(paths, json_output)` function
- **File**: `cli/daedalus/cli.py` — `scan` subparser with `paths` (nargs="*", default=["."]) and `--json`
- **File**: `cli/daedalus/_util.py` — `cache_dir()` moved here from `clean.py` (shared by scan + clean)
- **File**: `cli/daedalus/clean.py` — imports `cache_dir` from `_util` instead of defining locally
- **How it works**:
  - Recursively finds `.ere` files by extension + footer magic (`0xBEEFCAFE`)
  - Reads metadata from each file (reuses `format.read_footer()`)
  - Displays table: FILE, NAME, RUNTIME, ARCH, SIGNED, CREATED
  - Shows cache stats (entries + total size from `~/.cache/daedalus/`)
  - `--json` outputs structured JSON with all metadata fields
- **Exit codes**: 0 if files found, 1 if none found
- **Tested**: scan /tmp/ (found 4 files), scan --json, scan /nonexistent (exit 1), scan examples/ (exit 1) ✓

## New runtimes + release fix — 2026-07-20

### New runtimes: Go, PHP, Perl

- **File**: `cli/daedalus/runtimes/go.py` — Go runtime
  - Detection: `go.mod` in project root
  - Builds static binary via `go build`, embeds into .ere
  - Cross-compilation supported (GOOS/GOARCH)
- **File**: `cli/daedalus/runtimes/php.py` — PHP runtime
  - Detection: `composer.json` in project root
  - Framework detection: Laravel (artisan), Symfony (symfony.lock), WordPress (wp-config.php)
  - Entry point: public/index.php, index.php, bin/console, artisan
- **File**: `cli/daedalus/runtimes/perl.py` — Perl runtime
  - Detection: `Makefile.PL` or `cpanfile` in project root
  - Entry point: app.pl, bin/app, main.pl, server.pl, app.psgi
- **File**: `cli/daedalus/runtimes/__init__.py` — updated registry
  - Detection order: Python > Deno > Node > Java > Ruby > .NET > Go > PHP > Perl > Binary

### Unit tests

- **File**: `cli/tests/test_php_runtime.py` — 6 tests (detect, no-detect, Laravel, Symfony, WordPress, cross)
- **File**: `cli/tests/test_go_runtime.py` — 4 tests (detect, no-detect, no-go-on-path, cross)
- **File**: `cli/tests/test_perl_runtime.py` — 6 tests (detect Makefile.PL, detect cpanfile, no-detect, cross, app.pl entry, bin/app entry)
- **File**: `cli/tests/test_registry.py` — 5 tests (not-empty, all runtimes present, get_runtime, not-found, detection order)
- **File**: `cli/tests/conftest.py` — pytest path configuration
- **File**: `cli/pyproject.toml` — added `pytest>=7.0` to dev dependencies
- **Total**: 21 tests, all passing

### Release fix (critical bug)

- **Bug**: `release.yml` only packaged `daedalus-stub` + `daedalus-crypto` (Rust binaries), NOT the Python CLI (`daedalus`). Users could not run `daedalus` after installing from a release.
- **File**: `.github/workflows/release.yml` — restructured:
  - Packages full CLI bundle: Python package + Rust stubs + wrapper script
  - Naming: `daedalus-{os}-{arch}.tar.gz` (Bun/Wasmer pattern, no version in dir name)
  - SHA-256 checksums included
  - Release notes with changelog, install instructions, checksums section
- **File**: `scripts/install.sh` — updated to match new structure:
  - Expects `daedalus-{platform}/bin/daedalus` wrapper script
  - Handles both `sha256sum` (Linux) and `shasum` (macOS)
  - Installs Python CLI lib to `{INSTALL_DIR}/../lib/daedalus/python/`
  - Updates wrapper script with correct lib path
- **Architecture**: Rust CLI binary (`daedalus-cli`) built with `cargo`, installed to `target/release/daedalus`

### Documentation

- **File**: `README.md` — added Go, PHP, Perl to runtime table, guides, and quick links
- **File**: `docs/src/introduction.md` — updated runtime list
- **File**: `docs/src/SUMMARY.md` — added Go, PHP, Perl guide entries
- **File**: `docs/src/guides/go.md` — new guide page
- **File**: `docs/src/guides/php.md` — new guide page
- **File**: `docs/src/guides/perl.md` — new guide page

## Framework-specific detection — 2026-07-20

### Enhanced runtime detectors

**Node.js** (`cli/daedalus/runtimes/node.py`):
- Framework detection: Next.js (`next.config.js/mjs/ts`), Nuxt (`nuxt.config.ts/js/mjs`), Astro (`astro.config.mjs/ts`)
- Reads `scripts.start` from `package.json` as fallback entrypoint
- Next.js: entrypoint = `next start`
- Nuxt: entrypoint = `nuxt start`
- Astro SSR: entrypoint = `dist/server/entry.mjs` (after build) or `astro start`
- Generic: `main` field → `index.js`/`server.js`/`app.js`

**Python** (`cli/daedalus/runtimes/python.py`):
- Django detection: `manage.py` + `wsgi.py`/`asgi.py` in subdirectory
- Auto-finds gunicorn (WSGI) or uvicorn (ASGI) on PATH
- Fallback: `manage.py runserver 0.0.0.0:8000`
- Generic: `app.py`/`main.py`/`__main__.py`/`server.py`

**PHP** (`cli/daedalus/runtimes/php.py`):
- Laravel: `php artisan serve --host=0.0.0.0 --port=8000` (was just `artisan` which prints help)
- Symfony: `php bin/console server:run 0.0.0.0:8000`
- WordPress: `php -S 0.0.0.0:8080 -t /app` (PHP built-in server)
- Generic: `php -S 0.0.0.0:8000 -t /app/public`

**Hugo** (`cli/daedalus/runtimes/hugo.py`) — REWRITTEN RUNTIME:
- Detection: `hugo.toml`, `hugo.yaml`, `hugo.json`, `config.toml`/`config.yaml` (with Hugo-specific keywords)
- **Build phase**: runs `hugo --minify` during detect(), generates `public/` directory
- **Runtime**: serves static files via `python3 -m http.server 1313 --directory /app/public`
- Why: old `&&` entrypoint doesn't work with `execve()` (Linux doesn't support shell chaining in argv)
- Real-site test PASSED: `../tednoob17.github.io` (GoHugo blog) — 84 pages, 263 static files, 167MB after zstd (91MB images), build ~140s
- Hugo installed on system: `hugo v0.123.7+extended linux/amd64`
- Test file updated: `test_detect_with_hugo_binary` and `test_hugo_builds_and_serves` assertions fixed for new runtime design

### Unit tests

- **File**: `cli/tests/test_node_runtime.py` — added TestNextJsDetection (3), TestNuxtDetection (2), TestAstroDetection (2), TestNodeScriptsStart (1)
- **File**: `cli/tests/test_python_runtime.py` — added TestDjangoDetection (4)
- **File**: `cli/tests/test_php_runtime.py` — updated Laravel test (asserts `serve` + `--host`), WordPress test (asserts `-S`)
- **File**: `cli/tests/test_hugo_runtime.py` — NEW, 8 tests
- **Total**: 234 Python tests + 26 Rust tests = 260 tests, all passing

### Impossible cases (future work)

**WordPress** — Cannot package as single binary:
- Requires LAMP stack: Apache/Nginx + MySQL/MariaDB + php-fpm
- x.bin currently uses `php -S` built-in server as a fallback, but this is NOT production-ready
- For true WordPress support: would need to embed nginx + php-fpm + SQLite (or bundle MySQL)
- **Status**: documented, not implementable without a fundamentally different approach

**Vite** — Not a production runtime:
- Vite is a build tool / dev server, not a production application
- After `vite build`, output is static files in `dist/`
- x.bin could serve static files, but Vite itself is not the runtime
- **Status**: not applicable as standalone runtime

## .env file baking — 2026-07-20

- **File**: `cli/daedalus/dotenv.py` — NEW module
  - `parse_dotenv(env_file)`: parses KEY=value format (export prefix, quotes, comments, empty lines, values with `=`)
  - `detect_secret_keys(env)`: warns on `PASSWORD`, `SECRET`, `TOKEN`, `API_KEY`, `PRIVATE_KEY`, `CREDENTIALS` patterns
  - `load_dotenv(app_dir, env_file, verbose)`: resolves path relative to app_dir, parses, warns on secrets
- **File**: `cli/daedalus/cli.py` — `--env-file FILE` flag on build subcommand
- **File**: `cli/daedalus/build.py` — `env_file` param on `build()`, resolves to `env_file_path`, threads through `build_app_layer()` + `build_layers()`
- **File**: `cli/daedalus/layers.py` — `env_file_path` param on `build_app_layer()` and `build_layers()`:
  - Copies external `.env` file into app layer as `.env`
  - If `plan.env` is set (from daedalus.toml), writes a `.env` file with those key-value pairs
- **Flow**: `--env-file .env` → parse → merge into `plan.env` (set as real env vars by launcher) + copy file into app layer
- **Test file**: `cli/tests/test_dotenv.py` — 15 tests (parse_dotenv, detect_secret_keys, load_dotenv)
- **Status**: implemented, wired through build pipeline, tests passing

## Version metadata — 2026-07-20

- **File**: `cli/daedalus/cli.py` — `--version-info`, `--author`, `--description`, `--license` flags on build subcommand
- **File**: `cli/daedalus/build.py` — passes version/author/description/license to `build_meta_json()`
- **File**: `cli/daedalus/assembly.py` — `build_meta_json()` accepts and includes version/author/description/license in metadata JSON
- **File**: `cli/daedalus/inspect.py` — displays version/author/description/license when present
- **Flow**: `--version-info 1.0 --author "John"` → stored in `.ere` metadata JSON → displayed by `daedalus inspect`
- **Test file**: `cli/tests/test_version_metadata.py` — 6 tests
- **Status**: implemented, committed `961c526`

## Persistent storage — 2026-07-20

- **File**: `cli/daedalus/persistent.py` — NEW module
  - `get_persist_dir(app_name)` → `~/.local/share/daedalus/{app-name}/` (XDG compliant)
  - `ensure_persist_dir()` creates directory
  - `get_persist_env()` returns `{"DAEDALUS_PERSIST_DIR": "<path>"}`
- **File**: `cli/daedalus/cli.py` — `--persist` flag on build subcommand
- **File**: `cli/daedalus/build.py` — injects `DAEDALUS_PERSIST_DIR` into `plan.env` when `--persist` is set
- **Flow**: `--persist` → sets `DAEDALUS_PERSIST_DIR` env var → app reads it for persistent data
- **Test file**: `cli/tests/test_persistent.py` — 7 tests
- **Status**: implemented, committed `9872d53`

## Data files (--include) — 2026-07-20

- **File**: `cli/daedalus/cli.py` — `--include PATH` flag (repeatable, `action="append"`)
- **File**: `cli/daedalus/build.py` — resolves include paths relative to app_dir, validates existence
- **File**: `cli/daedalus/layers.py` — `build_app_layer()` and `build_layers()` accept `include_paths` param
- **Flow**: `--include data/config.json --include templates/` → copies files/dirs into app layer
- **Test file**: `cli/tests/test_include.py` — 6 tests (file, dir, multiple, none, overwrite, symlink)
- **Status**: implemented, committed `6ea54a5`

## Tree-shaking — 2026-07-20

- **File**: `cli/daedalus/treeshake.py` — NEW module
  - `detect_used_packages(app_dir)` → scans JS/TS source for require() and import statements
  - `prune_node_modules(app_dir)` → removes unused top-level packages from node_modules
- **File**: `cli/daedalus/cli.py` — `--tree-shake` flag on build subcommand
- **File**: `cli/daedalus/build.py` — runs `prune_node_modules()` before layer building
- **Flow**: `--tree-shake` → scan source → resolve used packages → remove unused from node_modules
- **Test file**: `cli/tests/test_treeshake.py` — 10 tests (detect, prune, scoped packages)
- **Status**: implemented, committed `0a1c5a9`

## Minification — 2026-07-20

- **File**: `cli/daedalus/minify.py` — NEW module
  - `minify_app_dir(app_dir)` → minifies JS/TS (via terser) and CSS (built-in stripper)
- **File**: `cli/daedalus/cli.py` — `--minify` flag on build subcommand
- **File**: `cli/daedalus/build.py` — runs `minify_app_dir()` before layer building
- **Flow**: `--minify` → scan app dir → minify JS/TS via terser, CSS via whitespace stripping
- **Test file**: `cli/tests/test_minify.py` — 7 tests (CSS, JS, skip node_modules, no files)
- **Status**: implemented, committed `74d2011`

## Framework auto-detect (enhanced) — 2026-07-20

- **File**: `cli/daedalus/runtimes/node.py` — enhanced `_detect_framework()`:
  - Config-file detection: Next.js, Nuxt, Astro, Remix, SvelteKit
  - Dependency-based detection: Express, Fastify, Hono (from package.json)
  - Entrypoint builders for Remix (`remix-serve build`), SvelteKit (`svelte-kit dev`), Express/Fastify (`node entry.js`), Hono (`node src/index.ts`)
- **File**: `cli/daedalus/runtimes/python.py` — added FastAPI and Flask detection:
  - `_detect_fastapi()` — scans source for `from fastapi import`, auto-selects uvicorn
  - `_detect_flask()` — scans source for `from flask import`
  - `_build_python_plan()` — shared builder for detected frameworks
- **Test file**: `cli/tests/test_framework_detect.py` — 15 tests (10 Node, 5 Python)
- **Status**: implemented, committed `c82c0d2`

## Health checks — 2026-07-20

- **File**: `cli/daedalus/health.py` — NEW module
  - `HealthState` class: mark_ready(), mark_not_ready(), uptime, version, extra fields
  - HTTP server: `/healthz` (liveness, always 200), `/readyz` (readiness), `/status` (JSON)
  - `start_health_server(port)` — background thread, daemon
  - `get_health_state()` — global singleton for app code
- **File**: `cli/daedalus/cli.py` — `--health-port PORT` flag on build subcommand
- **File**: `cli/daedalus/build.py` — injects `DAEDALUS_HEALTH_PORT` into plan.env
- **Flow**: `--health-port 8081` → launcher starts HTTP server → app marks ready via `daedalus.health.mark_ready()`
- **Test file**: `cli/tests/test_health.py` — 12 tests (state, server endpoints, disabled)
- **Status**: implemented, committed `0da1980`

## OpenTelemetry — DEPRECATED 2026-08-21

~~**File**: `cli/daedalus/otel.py`~~ — Python CLI module, removed. Rust equivalent (`daedalus-core/src/otel.rs`)
was **removed** in favor of simplicity (no metrics/export for a packaging tool).

~~**Flow**: `--otel-endpoint http://localhost:4317` → OTEL_* env vars set → app auto-instruments~~
~~**Status**: deprecated — use app-level OTel agents instead of daedalus embedding~~

## Cron/scheduled tasks — 2026-07-20

- **File**: `cli/daedalus/cron.py` — NEW module
  - `CronScheduler` — background thread, tick-based, @every/@hourly/@daily/@weekly + cron-style
  - `Task` dataclass — name, schedule, func, error tracking
  - `get_scheduler()` — global singleton
  - `build_cron_env()` — DAEDALUS_CRON_ENABLED + DAEDALUS_CRON_TASKS env vars
- **File**: `cli/daedalus/cli.py` — `--cron NAME:SCHEDULE` flag (repeatable)
- **File**: `cli/daedalus/build.py` — parses cron tasks, injects env vars
- **Flow**: `--cron cleanup:'*/5 * * * *'` → DAEDALUS_CRON_TASKS JSON → app registers tasks
- **Test file**: `cli/tests/test_cron.py` — 22 tests (schedule parsing, scheduler, error handling)
- **Status**: implemented, committed `465a3c9`

## Package manager support — 2026-07-20

### New module: `cli/daedalus/pkgmgr.py`

Automatic dependency installation via the user's package manager.

**Python package managers** (priority order):
| Manager | Lock file | Install command |
|---------|-----------|-----------------|
| uv | `uv.lock` | `uv sync` |
| poetry | `poetry.lock` | `poetry install --no-interaction` |
| pipenv | `Pipfile.lock` | `pipenv install --deploy` |
| pip | `requirements.txt` | `pip install -r requirements.txt` |

**Node package managers** (priority order):
| Manager | Lock file | Install command |
|---------|-----------|-----------------|
| pnpm | `pnpm-lock.yaml` | `pnpm install --frozen-lockfile` |
| yarn | `yarn.lock` | `yarn install --frozen-lockfile` |
| bun | `bun.lockb` | `bun install --frozen-lockfile` |
| npm | `package-lock.json` | `npm ci` |
| npm (no lock) | `package.json` | `npm install` |

**How it works**:
- `detect_pkgmgr(app_dir, runtime)` returns the first matching package manager
- `install_deps()` runs the install command in the app directory
- Integrated into `build()` pipeline: runs after runtime detection, before layer building
- `--no-install` flag skips automatic installation (for pre-installed deps)
- Incremental update hashes all lock files (not just `requirements.txt`)

**File**: `cli/daedalus/build.py` — added `no_install` param, pkgmgr integration
**File**: `cli/daedalus/cli.py` — added `--no-install` flag to build subcommand

### Unit tests

- **File**: `cli/tests/test_pkgmgr.py` — 17 tests
- **Total**: 38 tests, all passing

## Rust migration plan — 2026-07-20

### Strategy: layered migration, not rewrite

```
Phase 1 ✅            Phase 2 ✅            Phase 3 ✅
───────────────       ────────────          ──────────
daedalus-core crate       Full core lib         Full Rust CLI
format, compress      + assembly, sign,     daedalus-cli (all commands)
detect, pkgmgr        verify, tar, scan     workspace (3 crates)
                      + crypto binary       multi-arch release
                      + PyO3 bindings       no Python dependency
```

### Phase 1: daedalus-core crate — ✅ COMPLETE

**Modules**: format.rs, compress.rs, detect.rs, pkgmgr.rs
**Tests**: 40 Rust unit tests, all passing, clippy clean

### Phase 2: Full core lib — ✅ COMPLETE

daedalus-core now contains all build logic previously in Python:

| Rust module | Replaces | Tests |
|---|---|---|
| `format.rs` | `cli/daedalus/format.py` | 15 (footer parsing v2-v5, constants, read_at) |
| `compress.rs` | `cli/daedalus/layers.py` (zstd) | 3 (compress, decompress, roundtrip) |
| `detect.rs` | `cli/daedalus/analyzer/runtime.py` | 5 (11 runtimes: Python, Node, Deno, Java, Ruby, .NET, Go, PHP, Perl, Binary, Hugo) |
| `pkgmgr.rs` | `cli/daedalus/pkgmgr.py` | 5 (8 managers: uv, poetry, pipenv, pip, pnpm, yarn, bun, npm) |
| `tar.rs` | `cli/daedalus/layers.py` (tar) | 4 (create, deterministic, roundtrip) |
| `assembly.rs` | `cli/daedalus/assembly.py` | 5 (assemble, meta JSON, arch resolve, versioned footer) |
| `sign.rs` / `verify.rs` | `cli/daedalus/sign.py`, `verify.py` | integrated in build pipeline |
| `python.rs` | PyO3 bindings | exposed: format, compress, detect, pkgmgr |
| `crypto.rs` | `stub/src/bin/daedalus-crypto.rs` | keygen, sign, verify |

**Dependencies**: sha2, serde/serde_json, ruzstd, zstd, tar, ed25519-dalek, aes-gcm, hkdf

### Phase 3: Full Rust CLI — ✅ COMPLETE

**daedalus-cli** crate with 10 commands:

| Command | Description | Status |
|---|---|---|
| `daedalus build` | Package app into .ere | ✅ Full Rust |
| `daedalus inspect` | Read .ere footer metadata | ✅ |
| `daedalus scan` | Scan .ere for crypto/integrity info | ✅ |
| `daedalus sign` | Ed25519 sign a .ere | ✅ |
| `daedalus verify` | Verify signature | ✅ |
| `daedalus keygen` | Generate Ed25519 keypair | ✅ |
| `daedalus trust` | Add key to trusted dir | ✅ |
| `daedalus doctor` | Check dependencies | ✅ (--strict flag) |
| `daedalus env` | Show environment info | ✅ |
| `daedalus clean` | Clean build artifacts | ✅ |

**Workspace structure**:
```
Cargo.toml          (workspace root, [profile.release])
├── daedalus-core/      (shared library)
├── stub/           (self-extracting launcher, Linux-only)
└── daedalus-cli/       (CLI tool, cross-platform)
```

**Release workflow**: Multi-arch GitHub releases
- `linux-x64` (x86_64-unknown-linux-musl)
- `linux-arm64` (aarch64-unknown-linux-gnu)
- `macos-arm64` (aarch64-apple-darwin)
- `macos-x64` (x86_64-apple-darwin) — queued, runner slow

**Distribution**: Single binary, no `pip install` needed.
`curl -fsSL https://raw.githubusercontent.com/Tednoob17/daedalus/main/scripts/install.sh | bash`

### Benefits achieved

| Phase | Python needed? | Build speed | Distribution |
|-------|---------------|-------------|--------------|
| 1 ✅ | Yes | Same | Same |
| 2 ✅ | Yes (wrapper) | 2-5x faster | Same |
| 3 ✅ | **No** | **10-50x faster** | **Single binary** |

## daedalus-core wiring (Phase 1 complete) — 2026-07-20

Stub now uses `daedalus-core` as a shared library dependency instead of its local `format.rs`.

**Changes**:
- `stub/Cargo.toml` — added `daedalus-core = { path = "../daedalus-core" }`
- `stub/src/main.rs` — removed `mod format;`, replaced with `use daedalus_core::format::{self as format, read_at, Footer};`
- `stub/src/format.rs` — **deleted** (format parsing now comes from daedalus-core)
- `daedalus-core/src/format.rs` — fixed 3 clippy warnings (`doc_markdown`, `format_collect`, `unnecessary_map_or`)

**Verification**:
- `stub` compiles clean with `cargo check` and `cargo clippy -- -D warnings`
- `daedalus-core` — 26/26 tests pass, clippy clean
- All existing `format::FLAG_SIGNED`, `format::CRYPTO_AES_256_GCM`, `format::PAYLOAD_FORMAT_SQUASHFS` references work unchanged via `self as format` alias

**What this enables**:
- Phase 2: Python CLI can call daedalus-core via PyO3 or subprocess
- Phase 3: Full Rust CLI — both stub and CLI share the same format parser
- Single source of truth for .ere format (no more duplicate format.rs)

## Install script + upgrade command — 2026-07-20

- **File**: `scripts/install.sh` — curl-pipe-bash installer
  - Detects platform (linux-x64, linux-arm64, macos-x64, macos-arm64)
  - Fetches latest version from GitHub API
  - Downloads tar.gz from releases, verifies SHA-256 checksum
  - Installs to `/usr/local/bin/` (or `$DAEDALUS_INSTALL_DIR`)
  - Idempotent: skips if already up-to-date
  - Usage: `curl -fsSL https://raw.githubusercontent.com/Tednoob17/daedalus/main/scripts/install.sh | bash`
- **File**: `cli/daedalus/upgrade.py` — `daedalus upgrade` self-update command
  - Fetches latest version from GitHub API
  - Compares against current `_DAEDALUS_VERSION`
  - Downloads platform-specific tar.gz, verifies SHA-256
  - Replaces daedalus binary in-place (with sudo if needed)
- **File**: `cli/daedalus/cli.py` — `upgrade` subparser + dispatch
- **File**: `.github/workflows/release.yml` — fixes:
  - Removed stale `RUST_VERSION: "1.80"` env var (never used, rustc is 1.97.1)
  - Release notes now point to `scripts/install.sh` for easy install
  - SHA-256 checksum files now uploaded as release assets
  - Removed redundant `generate_release_notes: true` (was conflicting with `body_path`)
- **Tested**: install.sh syntax check ✓, import ✓, help ✓, build+inspect+scan ✓

## SquashFS support — 2026-07-18

- **File**: `stub/src/format.rs` — format v5, `PAYLOAD_FORMAT_SQUASHFS = 2`
- **File**: `stub/src/main.rs` — squashfs extraction via `squashfs_extract::extract()`, uses backhand crate
- **File**: `stub/src/squashfs_extract.rs` — backhand-based squashfs reader (gzip/lz4/zstd support)
- **File**: `cli/daedalus/build.py` — `--squashfs` flag, `mksquashfs` build, tar→squashfs conversion
- **File**: `docs/src/reference/format.md` — v5 format documented
- Metadata `"payload_format": "squashfs"` tells launcher to use squashfs extraction instead of zstd(tar)
- **Note**: SquashFS is a better-compressed layer format (vs zstd+tar). Extraction to disk still happens at startup. Direct mmap without extraction (the real cold-start perf win) is a Phase 3 goal — see "Next steps".

## Ed25519 verification: implemented

- `stub/src/main.rs:70-75` — calls `verify_ed25519()` when `format_version >= 3 && flags & FLAG_SIGNED`.
- `stub/src/main.rs:119-182` — full verification logic: reads sig block at `sig_offset`,
  computes SHA-256(payload ‖ meta_bytes), iterates trusted keys from `$XDG_DATA_HOME/daedalus/trusted-keys/`
  (legacy fallback: `~/.ere/trusted-keys/`), verifies via `ed25519_dalek::Verifier`.
- `stub/Cargo.toml:18` — `ed25519-dalek` with `default-features = false, features = ["alloc"]`.

## Keygen / Sign / Verify CLI: IMPLEMENTED

All implemented in the session of 2026-07-09:

- `stub/Cargo.toml` — added `[[bin]]` target `daedalus-crypto`. Also added `rand = "0.8"`.
- `stub/src/bin/daedalus-crypto.rs` — three subcommands:
  - `keygen --key-dir <dir>`: generate Ed25519 keypair, write `{fingerprint}.key` (32-byte seed)
    and `{fingerprint}.pub` (32-byte pubkey), print hex fingerprint to stdout.
  - `sign <keyfile>`: read 32-byte SHA-256 hash from stdin, sign, write 64-byte sig to stdout.
    Exit 0 = success, 1 = error.
  - `verify <pubkey>`: read 96 bytes from stdin ([32-byte hash][64-byte sig]), verify.
    Exit 0 = valid, 1 = invalid, 2 = error.
- `cli/daedalus/crypto.py` — `find_crypto()` (mirrors `find_stub()`) + thin subprocess wrappers
  for keygen/sign/verify.
- `cli/daedalus/keygen.py` — `daedalus keygen` CLI (default dir `$XDG_DATA_HOME/daedalus/keys`, legacy fallback `~/.ere/keys`).
- `cli/daedalus/sign.py` — `daedalus sign <file.ere>`: reads file with format.py, computes
  SHA-256(payload‖meta), calls crypto.py sign, writes sig_block `[sig_size:u32le][64-byte sig]`
  between metadata and footer, rewrites footer as v3 (format_version=3, flags|=FLAG_SIGNED,
  sig_offset set, footer grown to 92 bytes). In-place modification.
- `cli/daedalus/verify.py` — `daedalus verify <file.ere>`: reads v3 footer, iterates trusted keys
  from `$XDG_DATA_HOME/daedalus/trusted-keys/` (legacy fallback `~/.ere/trusted-keys/`, or `--trusted-dir`), calls crypto.py verify for each.
- `cli/daedalus/cli.py` — wired up keygen/sign/verify subcommands.
- `Makefile` — `make stub` builds both `daedalus-stub` and `daedalus-crypto`.
- `.cargo/config.toml` — target-dir is `/tmp/daedalus-stub-target` (vfat workaround).
- `find_stub()` and `find_crypto()` now also search `/tmp/daedalus-stub-target/`.

## End-to-end test results

```
$ cargo build -p daedalus-cli && ./target/release/daedalus build examples/hello-web -o /tmp/hello-web.ere       → OK (7.1MB)
$ ./target/release/daedalus keygen --key-dir /tmp/daedalus-keys                        → OK, fingerprint printed
$ ./target/release/daedalus sign /tmp/hello-web.ere --key <keyfile>               → OK, sig_offset=7117820
$ ./target/release/daedalus verify /tmp/hello-web.ere --trusted-dir /tmp/daedalus-trusted → OK, exit 0
$ dd if=/dev/urandom of=/tmp/hello-web.ere bs=1 seek=688788 count=1    → corrupt payload
$ ./target/release/daedalus verify /tmp/hello-web.ere --trusted-dir /tmp/daedalus-trusted → FAIL, exit 1 (no crash)
```

## Design audit fixes (2026-07-17)

All 14 issues from the design audit have been fixed. Changes verified via Python import tests and pack/unpack round-trip tests.

### Fix #1: inspect.py fingerprint (removed wrong logic)
**File:** `cli/daedalus/inspect.py:39-45`
**What changed:** Removed incorrect `hashlib.sha256(sig[:32])` fingerprint computation — it was hashing 32 arbitrary bytes from the Ed25519 signature, not the actual public key. Replaced with actionable guidance: "run 'daedalus verify'". Removed unused `hashlib` import. Now uses `fmt.SIG_BLOCK_SIZE` constant instead of hardcoded `68`.

### Fix #2: format.py sig-block constants (eliminated struct duplication)
**Files:** `cli/daedalus/format.py`, `cli/daedalus/build.py`, `cli/daedalus/sign.py`, `cli/daedalus/verify.py`, `cli/daedalus/inspect.py`
**What changed:** Added `SIG_BLOCK_SIZE = 68`, `SIG_BLOCK_SIZE_FIELD = 64`, `pack_sig_block()`, `unpack_sig_block()` to `format.py`. Updated `sign.py` and `verify.py` to use these helpers (removed `import struct` from both). Updated `inspect.py` to use `fmt.SIG_BLOCK_SIZE`. Removed unused `import struct` from `build.py`. All sig-block logic is now in one place (format.py) matching Rust's `format.rs`.

### Fix #3: cli.py RuntimeError catch (was missing)
**File:** `cli/daedalus/cli.py:103`
**What changed:** Added `RuntimeError` to the catch list. `build.py` raises `RuntimeError` at lines 250 and 254 when pip subprocess fails; these were uncaught and would show raw tracebacks to users.

### Fix #4+6: main.rs LD_LIBRARY_PATH + env duplication (DRY refactor)
**File:** `stub/src/main.rs:22-26, 321-386`
**What changed:** Extracted `const LD_PATHS` (lines 24-26), `enter_namespace_if_needed()` (322-327), `setup_env()` (331-366), `make_resolve()` (369-379), and `env_to_cstrings()` (382-386). Both `exec_app()` and `supervise_services()` now call these shared functions instead of duplicating env setup, LD_LIBRARY_PATH construction, and namespace entry logic. LD_LIBRARY_PATH dirs were defined in 4+ places; now in one constant.

### Fix #5: SHA-256 contract documentation
**Files:** `stub/src/format.rs`, `cli/daedalus/format.py`
**What changed:** Added cross-reference doc comments in both Rust and Python documenting the integrity hash contract: `SHA-256(payload ‖ metadata_json)`. Also corrected stale wording from `SHA-256(layers ‖ metadata)` to `SHA-256(payload ‖ metadata)` (the old wording was misleading — the hash is over the full payload, not just layers).

### Fix #7: Layer.usize rename (descriptive field name)
**File:** `stub/src/main.rs:59-68`
**What changed:** Renamed `Layer.usize` to `Layer.uncompressed_size` with `#[serde(rename = "usize")]` for JSON compat. Removed `#[allow(dead_code)]` — the field is now descriptively named and actively used.

### Fix #8: _sign_and_write helper (DRY in build.py)
**File:** `cli/daedalus/build.py`
**What changed:** Extracted `_sign_and_write()` helper function for the inline-sign-then-write-footer pattern that was duplicated in two places. Removed unused `pub_path` variable.

### Fix #9: _ManifestPlan dead field (removed extra_dirs_host)
**File:** `cli/daedalus/build.py`
**What changed:** Removed the unused `extra_dirs_host` field from `_ManifestPlan` dataclass.

### Fix #10: sys.exit → ValueError (proper error flow through cli.py)
**Files:** `cli/daedalus/sign.py`, `cli/daedalus/verify.py`, `cli/daedalus/trust.py`
**What changed:** Converted all `sys.exit(1)` / `sys.exit(0)` to `raise ValueError(...)` (or `return` for the "already trusted" case in trust.py). Errors now flow through `cli.py`'s catch list and get formatted as `[daedalus] error: ...` instead of ugly tracebacks or silent exits. Added `try/except FileNotFoundError` around `os.listdir(keys_dir)` in sign.py.

### Fix #11: find_binary extraction (shared utility)
**Files:** `cli/daedalus/_util.py` (new), `cli/daedalus/build.py`, `cli/daedalus/crypto.py`
**What changed:** Created `_util.py` with shared `find_binary()` function (searches PATH + Cargo target dir). Updated `build.py` and `crypto.py` to import from `_util.py` instead of duplicating binary-finding logic. No circular imports — `_util` is a leaf module.

### Fix #12: French → English comments
**File:** `cli/daedalus/analyzer/runtime.py:25, 59`, `cli/daedalus/cli.py:71`
**What changed:** Translated three French comments to English: "argv relatif au rootfs" → "argv relative to rootfs", "On embarque les site-packages..." → "Embed site-packages...", "remplace le process" → "replaces the process".

### Fix #13: ldd.py inlined to elf.py
**Files:** `cli/daedalus/analyzer/ldd.py` (reduced to 2-line re-export), `cli/daedalus/build.py`
**What changed:** `ldd.py` was just a one-function wrapper around `elf.shared_libs`. Reduced it to a 2-line backwards-compat re-export from `elf.py`. Updated `build.py` to import from `elf` directly, 3 call sites changed from `ldd.shared_libs` to `elf.shared_libs`.

### Fix #14: __pycache__ cleanup
**What changed:** Removed stale `__pycache__` directories that contained cpython-312 .pyc files alongside cpython-313 ones. Clean builds will regenerate only the correct version.

### Verification
- All Python modules import cleanly with no circular dependencies
- `format.py` pack/unpack sig-block round-trip test passes
- `cargo build --release` + `cargo clippy -- -D warnings` passent clean (0 warnings, 0 errors)
- No test suite exists; `make example` should build a working .ere to confirm end-to-end

## Clig.dev CLI audit (2026-07-20)

Full audit against https://clig.dev — 12 gaps identified, 11 commits, all fixed.

| # | Gap | Fix | Commit |
|---|-----|-----|--------|
| 1 | Progress on stdout | All `print()` → `file=sys.stderr` (build, clean, lockfile, fetch, cross) | `348e0b4` |
| 2 | `clean --all` no confirmation | Interactive prompt + `-f`/`--force` flag, non-interactive requires `--force` | `fe8aaba` |
| 3 | Silent network fetches | "detecting dependencies..." + "downloading N dependencies..." messages | `9ba1473` |
| 4 | Non-XDG key paths | `$XDG_DATA_HOME/daedalus/` with legacy `~/.ere/` fallback + deprecation warning | `8a2bafe` |
| 5 | No `--version` | `daedalus --version` → `daedalus 0.1.0` | `f519099` |
| 6 | No machine-readable output | `--json` flag for `inspect` and `doctor` | `9b567f3` |
| 7 | Subcommand abbreviation | Already prevented by argparse (Python 3.12) | N/A |
| 8 | No color support | `_color.py` module: `--no-color`, `NO_COLOR`, `TERM=dumb`, isatty detection | `b36e086` |
| 9 | No isatty checks | `verbose = not args.quiet and sys.stderr.isatty()` — auto-suppress in pipes | `7ae378b` |
| 10 | No help examples | Epilog with 6 usage examples + docs link | `a64dcfd` |
| 11 | No `help` subcommand | `daedalus help [command]` via `_SUBPARSERS` dict dispatch | `5652e83` |
| 12 | No exit code docs | 0/1/2 documented in `--help` epilog | `2158078` |

### New files
- `cli/daedalus/_color.py` — ANSI color helpers (red, green, yellow, bold) with TTY/NO_COLOR/dumb detection

### Key changes
- `_util.py`: added `keys_dir()` and `trusted_dir()` functions (XDG + legacy fallback)
- `cli.py`: `--no-color` global flag, `--version`, `_SUBPARSERS` dict, `_verbose` with isatty, `help` subcommand, `RawDescriptionHelpFormatter` epilog
- `build.py`: stderr for all progress, network fetch transparency messages, removed dead `_NO_KEYS_MSG`
- `clean.py`: `--force` flag, interactive confirmation for `--all`
- `inspect.py`: refactored with `_collect_inspect_data()` helper, `--json` output
- `doctor.py`: refactored with `_collect_checks()` helper, `--json` output, colored check markers
- `sign.py`, `verify.py`, `trust.py`, `keygen.py`: updated to use `_util.keys_dir()` / `_util.trusted_dir()`
- `lockfile.py`, `fetch.py`, `cross.py`: stderr for all progress messages

## Real-app testing — URGENT (all runtimes)

**Why**: Unit tests pass but we've never tested with real apps. Toy examples hide real bugs — missing shared libs, wrong entrypoints, broken dep resolution, env issues. We need to prove x.bin works on production apps for every runtime.

**Process per app**: `git clone` → `daedalus build` → run → document pass/fail/bugs → fix → re-test

### Testing matrix (one real app per runtime, user will send apps)

| Runtime | App type | What to test | Status |
|---------|----------|-------------|--------|
| **Python** | Flask/FastAPI web app | detect → build → serve → HTTP 200 | NEED APP |
| **Node.js** | Express/Next.js app | detect → build → serve → HTTP 200 | NEED APP |
| **Deno** | Fresh/Todo app | detect → build → serve → HTTP 200 | NEED APP |
| **Java** | Spring Boot / Maven app | detect → build → serve → HTTP 200 | NEED APP |
| **Ruby** | Sinatra/Rails app | detect → build → serve → HTTP 200 | NEED APP |
| **.NET/C#** | ASP.NET app | detect → build → serve → HTTP 200 | NEED APP |
| **Go** | Caddy/Traefik-like app | detect → build → run → HTTP 200 | NEED APP |
| **PHP** | Laravel/Symfony app | detect → build → serve → HTTP 200 | NEED APP |
| **Perl** | Mojolicious/Dancer app | detect → build → serve → HTTP 200 | NEED APP |
| **Hugo** | tednoob17.github.io | detect → build → serve → HTTP 200 | ✅ DONE |
| **Binary** | Static ELF binary | detect → build → run → correct output | NEED APP |

### Known issues to expect
- **Python**: site-packages missing, venv not fully captured, shared lib resolution failures
- **Node.js**: node_modules too large, native addons (node-gyp) fail
- **Deno**: vendor cache not captured, missing deno binary
- **Java**: missing JVM, JAR manifest wrong, classpath issues
- **Ruby**: gem paths wrong, bundler not captured
- **.NET**: dotnet runtime missing, publish dir empty
- **Go**: static binary OK but dynamic linking issues possible
- **PHP**: php-fpm not available, composer autoload wrong
- **Perl**: perl not captured, module paths wrong
- **Hugo**: ✅ works (tednoob17.github.io test passed)
- **Binary**: musl vs glibc mismatch, missing shared libs

### What user will send
- One real app per runtime (or a few) — user selects which ones to test first
- Each app gets a full build → run → fix cycle
- Results tracked in `TESTED_APPS.md` (pass/fail, size, notes, bugs found)

## Next steps (future)

### Real-app testing — top 200 GitHub projects (HIGH PRIORITY)
- **Goal**: prove x.bin works on real-world apps, not just toy examples
- **Approach**: test `daedalus build` against top 200 GitHub repos (by stars), curate the ones that work as prebuilt downloads
- **Target repos to test** (Python/Node.js focus, apps not libraries):
  - **Python web**: flask (pallets/flask), fastapi (tiangolo/fastapi), django (django/django), sanic (sanic-org/sanic), litestar (litestar-org/litestar)
  - **Python tools**: httpie (httpie/cli), httpx (encode/httpx), thefuck (nvbn/thefuck), borgbackup (borgbackup/borg), pgcli (dbcli/pgcli), mycli (dbcli/mycli), ranger (ranger/ranger), streamlink (streamlink/streamlink), you-get (soimort/you-get), yt-dlp (yt-dlp/yt-dlp), tldr (tldr-pages/tldr)
  - **Python data**: jupyter (jupyter/jupyter), numpy (numpy/numpy), pandas (pandas-dev/pandas), matplotlib (matplotlib/matplotlib), scikit-learn (scikit-learn/scikit-learn), polars (pola-rs/polars)
  - **Python infra**: ansible (ansible/ansible), fabric (fabric/fabric), invoke (pyinvoke/invoke), salt (saltstack/salt)
  - **Node.js**: express (expressjs/express), next.js (vercel/next.js), n8n (n8n-io/n8n), Ghost (TryGhost/Ghost), PM2 (Unitech/pm2), homebridge (homebridge/homebridge), mosca (moscajs/mosca)
  - **Go (future)**: caddy (caddyserver/caddy), traefik (traefik/traefik), hugo (gohugoio/hugo), lazygit (jesseduffield/lazygit)
- **Process**: for each repo, `git clone` → `daedalus build` → test run → document what works/breaks
- **Distribution**: working builds become official downloads on the website (`daedalus.sh/downloads`)
- **Why**: marketing proof point ("we can build Flask, FastAPI, yt-dlp, n8n…"), real-world bug discovery, performance benchmarks
- **File**: track results in `TESTED_APPS.md` at repo root (pass/fail, size, notes)

### Distribution & packaging
- GitHub Actions official action (`action-daedalus/build`) — for CI/CD workflows

### Remaining features
- Cross-build aarch64 stub locally: requires `rustup target add aarch64-unknown-linux-musl` + cross-linker. CI handles this automatically via GitHub Actions runners.
- `daedalus sign` with automatic key lookup in `$XDG_DATA_HOME/daedalus/keys/` (without `--key`).
- `squashfs + mmap` direct read (kernel mount, Linux 5.12+, no extraction needed) — the real cold-start perf win beyond just better compression.
- LRU cache cleanup (evict beyond threshold)
- Cold/warm start < 100 ms end-to-end
- Distribution / discovery (lightweight registry)
- Run full end-to-end build+sign+verify cycle for aarch64 once stub is compiled locally
- GitHub Actions official action (`action-daedalus/build`) — for CI/CD workflows

### Competitor feature gaps (x.bin vs Bun vs Deno Deploy)

| Feature | Bun | Deno Deploy | x.bin | Priority | Status |
|---------|-----|-------------|-------|----------|--------|
| `.env` file baking | Built-in `.env` | `.env` per playground | ✅ Implemented | HIGH | DONE |
| Version metadata in binary | ✅ | ✅ | ✅ Implemented | HIGH | DONE |
| Persistent storage | `Bun.sql` | `Deno.openKv()` | ✅ `--persist` flag | HIGH | DONE |
| Data files (--include) | ✅ | ✅ Deno KV | ✅ `--include` flag | HIGH | DONE |
| Tree-shaking | `--exclude-unused-npm` | ✅ | ✅ `--tree-shake` | HIGH | DONE |
| Minification | `--bundle --minify` | ✅ | ✅ `--minify` | HIGH | DONE |
| Framework auto-detect | ✅ | ✅ `deno compile .` | ✅ Enhanced | HIGH | DONE |
| Hot reload (dev mode) | `bun --hot` | Tunnels + HMR | ❌ Missing | LOW | TODO (not production-focused) |
| Browser target | `--compile --target=browser` | N/A | ❌ N/A | LOW | N/A (different use case) |
| Cron/scheduled tasks | `Bun.cron()` | Cron in dashboard | ✅ `--cron` flag | MEDIUM | DONE |
| Health checks | N/A | Built-in | ✅ `--health-port` | MEDIUM | DONE |
| OpenTelemetry | N/A | Auto-instrumented | ✅ `--otel-endpoint` | MEDIUM | DONE |

### Competitive analysis: x.bin vs Bun vs Wasmer (2026-07-23)

**Bun (`bun build --compile`)**:
- Compiles JS/TS into standalone executable, embeds entire Bun runtime (~50MB)
- Self-contained: target needs nothing installed
- Bytecode compilation for 2x faster startup
- Cross-compilation: 8 targets (linux/win/mac × x64/arm64, musl variants)
- Full-stack executables (HTML/CSS/JS frontend + server)
- Embeds files (images, configs, SQLite DBs), code signing (macOS), minification, sourcemaps
- Massive API surface: HTTP, WebSocket, PostgreSQL, Redis, SQLite, S3, FFI

**Wasmer**:
- WebAssembly runtime — runs any language compiled to .wasm
- Universal: Rust, Go, Python, C, Ruby, JS — all compile to WASM
- Cloud/edge deployment platform (Wasmer Edge) + registry (wasi.dev)
- Multiple compilation backends: Singlepass, LLVM, Cranelift, JavaScriptCore
- SDKs in Rust, Python, JS, Go, Ruby, C
- Security: sandboxed execution, metering (instruction limits)

**Where x.bin is unique**:
- Packaging sans recompilation — Bun oblige à re-bundler, Wasmer oblige à recompiler en WASM. x.bin prend l'app telle quelle.
- Tiny overhead — Bun embarque ~50MB de runtime, x.bin ajoute ~100KB de stub.
- Intégrité + signature — SHA-256 + Ed25519 sign/verify + chiffrement v4. Bun n'a que codesign macOS. Wasmer a le sandbox.
- Cache intelligent — Si le hash est déjà extrait, on saute l'extraction.
- Multi-langue — Python, Node, Go, Rust, PHP… sans changer une ligne de code.

**Where x.bin is weaker**:
- Bun's `--compile` produces self-contained binaries (no runtime needed on target). x.bin requires python3/node/etc. on target.
- Bun has bytecode compilation, cross-compilation for the app itself, full-stack HTML embedding, minification, sourcemaps.
- Wasmer has cloud deployment, WASM universality, registry, metering, SDKs in 6 languages.

### Improvement roadmap based on competitive analysis (2026-07-23)

| # | Improvement | Inspired by | Priority | Effort |
|---|-------------|-------------|----------|--------|
| 1 | **Embedded runtime option** — optionally bundle python3/node/etc. into the binary for targets without the runtime installed | Bun's self-contained approach | HIGH | LARGE |
| 2 | **Cross-compilation for apps** — build for aarch64 from x86_64 (stub already does this, extend to app layers) | Bun's 8-target cross-compilation | HIGH | MEDIUM |
| 3 | **Bytecode precompilation** — precompile Python `.pyc` or Node bytecode at build time for faster startup | Bun's `--bytecode` flag | MEDIUM | MEDIUM |
| 4 | **Registry/manifest** — `daedalus publish` + `daedalus install <package>` for sharing apps, like wasi.dev | Wasmer Registry | MEDIUM | LARGE |
| 5 | **Cloud deploy** — `daedalus deploy` to a backend (Wasmer Edge-like) | Wasmer Edge platform | LOW | VERY LARGE |
| 6 | **WASM support** — package .wasm + wasmer runtime in the binary | Wasmer's universal approach | LOW | VERY LARGE |
| 7 | **Minification** — JS/CSS minification for Node apps at build time | Bun's `--minify` | MEDIUM | SMALL |
| 8 | **Sourcemaps** — embed sourcemaps for better error reporting | Bun's `--sourcemap` | LOW | SMALL |
| 9 | **Full-stack HTML** — embed HTML/CSS/JS frontend + server in one binary | Bun's full-stack executables | LOW | MEDIUM |
| 10 | **Windows support** — extend beyond Linux/macOS | Bun's 3-OS support | MEDIUM | LARGE |
| 11 | **Hot reload** — `daedalus dev` for development mode | Bun's `--hot` | LOW | MEDIUM |
| 12 | **Metering/instruction limits** — deterministic resource limits for sandboxed apps | Wasmer's metering | LOW | MEDIUM |

### Hugo real-site test — ✅ DONE
- **Site**: `../tednoob17.github.io` — GoHugo site with risotto theme
- **Result**: PASSED — 84 pages, 263 static files, Hugo v0.123.7+extended builds `public/` in ~1s, zstd compression ~140s
- **Binary size**: 167MB after zstd (91MB is images — compressible further with image optimization)
- **Runtime**: python3 -m http.server 1313 serves static files at runtime (hugo --minify runs at build time)

## Seccomp BPF denylist (2026-07-17)

- **File**: `stub/src/main.rs` — `install_seccomp_denylist()`.
- **Approach**: Denylist (not allowlist). Conservative: ~14 syscalls blocked, everything else allowed.
- **Rationale**: Python/Node.js use 150+ distinct syscalls. An allowlist would break apps unpredictably. A denylist of clearly dangerous syscalls is sufficient — namespace isolation handles the rest.
- **Blocked syscalls**: ptrace, mount, umount2, pivot_root, reboot, kexec_load, kexec_file_load, init_module, finit_module, delete_module, swapon, swapoff, sethostname, setdomainname, acct, nfsservctl.
- **Hook point**: After `pivot_root_into()` in both `exec_app()` (single-service) and `supervise_services()` (multi-service). Filter applies before `execve()` — child processes inherit it.
- **Graceful degradation**: If `prctl(PR_SET_SECCOMP)` fails, prints `[daedalus] warning` to stderr and continues. Never blocks execution.
- **BPF program**: 21 instructions. Architecture check (reject non-x86_64), then 16 syscall comparisons with forward-jump chain to KILL at instruction [19]. ALLOW at instruction [20].
- **x86_64 only**: Syscall numbers are hardcoded for x86_64 (`asm/unistd_64.h`). Cross-arch (aarch64) is Phase 3.
- **No new crate**: Uses `libc = "0.2"` only. Raw BPF structs (`libc::sock_filter`, `libc::sock_fprog`) + `libc::prctl`.
- **Docs updated**: `security.md` (new section 5, status line), `isolation.md` (level 2 mechanism), `roadmap.md` (marked done).

## CODE_STYLE.md and enforcement (2026-07-17)

- `CODE_STYLE.md` written at repo root — philosophy of 42/Epitech Norm + Linux kernel style adapted for Rust/Python.
- **Rust**: 40-line function guideline, clippy::pedantic subset, SAFETY comments on all unsafe blocks.
- **Python**: Black (88 cols) + ruff (E/W/F/I/UP/B/SIM/RUF), 60-line function guideline, mandatory type hints.
- Enforcement: `make lint` / `make fmt` targets (no CI yet).

### Config files added
- `cli/pyproject.toml` — ruff + black config (py313 target).
- `stub/Cargo.toml` — `[lints.clippy]` section (pedantic subset).
- `stub/rustfmt.toml` — 100-col width.
- `Makefile` — lint/rust, lint/python, fmt/rust, fmt/python targets.

### CODE_STYLE.md fixes applied (all files)
- **format.py**: removed duplicate SIG_BLOCK_SIZE/SIG_BLOCK_SIZE_FIELD constants (bug from design audit). Black formatting. Committed `b8f6cc6`.
- **sign.py**: extracted `_resolve_signing_key()`, `_write_signed()` — `sign()` 65→30 lines. Removed WHAT comments. Committed `b8f6cc6`.
- **verify.py**: removed WHAT comments. Committed `b8f6cc6`.
- **inspect.py**: Black formatting, fixed f-string without placeholders (ruff F541). Committed `b8f6cc6`.
- **build.py**: extracted 10+ helpers (`_resolve_app_path`, `_resolve_service_binary`, `_collect_service_bins`, `_build_meta_json`, `_assemble_daedalus`, `_build_layers`, `_copy_service_layers`, `_copy_app_files`, `_install_manifest_pip`, `_build_service_metadata`) — `_build_manifest` 171→35, `build` 138→40 lines. Committed `c7e402e`.
- **trust.py**: removed unused `import os` (ruff F401). Committed `4d938d3`.
- **crypto.py**: Black formatting. Committed `4d938d3`.
- **cli.py**: Black formatting. Committed `4d938d3`.
- **runtime.py**: extracted `_detect_python()`, `_detect_node()` — `detect()` 68→25 lines. Committed `c92e114`.
- **elf.py**: removed unused `import os` (F401), removed dead `sub_dirs` assignment (F841). Bug fix: `_resolve_recursive` now carries per-library search_dirs in queue `(name, dirs)` so each library resolves its own deps via DT_RUNPATH. Committed `c92e114` + `917b059`.
- **format.rs**: removed WHAT-style numbered comments. Committed `e56146e`.
- **main.rs**: added SAFETY comments to all 8 unsafe blocks, split `supervise_services` (104→15 lines) into `fork_services`(45), `wait_for_health`(14), `wait_for_children`(40). Committed `fae9f56`.

## Dockerfile dependency analyzer: Feature A (2026-07-17)

- **File**: `cli/daedalus/analyzer/dockerfile.py` — committed `ebea282`.
- **Public API**: `detect_from_dockerfile(app_dir: Path) -> list[DetectedDep]`
- **DetectedDep** dataclass: `kind` ("pip"/"npm"/"apt"/"apk"/"external"), `name`, `version`, `url` (for external), `source`.
- Parses Dockerfile RUN instructions with join-then-split architecture:
  1. Strip comments, join `\` line continuations
  2. Extract full RUN blocks (no premature `&&` splitting)
  3. Try multi-step chain detection first (wget/curl → tar/unzip → chmod +x)
  4. Fall back to split-on-`&&` for individual package pattern matching
- Patterns: `apt-get install`, `apk add`, `pip install`, `npm install -g`
- External binary fetch: state chain detection (FETCHED → EXTRACTED → CHMODDED) with URL + semver extraction
- No Dockerfile → returns `[]`, no error. Graceful degradation.
- No duplication with existing `requirements.txt`/`package.json` handling in build.py/runtime.py.

## Docs rewrite (2026-07-17)

- `docs/src/reference/format.md` — fully rewritten to English with constraint→options→choice for v3 prefix trick.
- `docs/src/security.md` — fully rewritten with concrete examples and attack/defense structure.
- `docs/src/concepts/problem.md`, `positioning.md`, `architecture.md` — already English, no changes needed.
- `docs/src/reference/builder.md`, `launcher.md`, `cache.md`, `isolation.md` — already English, no changes needed.
- `docs/src/guides/quickstart.md`, `python.md`, `node.md`, `dependencies.md` — already English, no changes needed.
- mdbook builds clean. `.github/workflows/docs.yml` added for GitHub Pages deploy.

## Feature B: Python source AST scanner (2026-07-17)

- **File**: `cli/daedalus/analyzer/python_ast.py` — committed `638b868`.
- **Public API**: `detect_from_python_source(app_dir: Path) -> list[DetectedDep]`
- **Merge utility**: `merge_deps(dockerfile_deps, ast_deps) -> list[DetectedDep]`
- Walks `ast.Call` nodes for `subprocess.run/Popen/call/check_call/check_output/getoutput/getstatusoutput` and `os.system/os.popen`.
- Extracts literal binary names from string args (`"ffmpeg -i ..."`) and list args (`["ffmpeg", ...]`).
- Variables/expressions flagged as `confidence="uncertain"`.
- Skips builtins (python, node, bash, sh, etc.).
- Dedup merge: Dockerfile wins over AST for same binary name.
- Python only for now. Node.js gap documented (requires JS parser).

## Feature C: dependency fetcher into staging (2026-07-17)

- **File**: `cli/daedalus/analyzer/fetch.py` — committed next.
- **Public API**: `fetch_deps(deps, verbose) -> (stage_dir, list[FetchResult])`
- **FetchResult** dataclass: `dep`, `ok`, `error`, `sha256`.
- Staging directory: `~/.cache/daedalus/stage/{SHA-256 of sorted dep list}/` with subdirs per kind.
- Fetchers per dependency type:
  - **pip**: `pip download --no-deps --dest {stage}/pip/` (never installs globally)
  - **npm**: `npm install --prefix {stage}/npm/ --save=false` (never touches global node_modules)
  - **apt**: `apt-get download` + `dpkg-deb -x` into staging (never `apt-get install`)
  - **apk**: `apk fetch --simulate --stdout` + write/extract .apk into staging
  - **external**: `urllib.request.urlretrieve` + extract archive into staging
- Checksum handling: SHA-256 recorded in `manifest.json` for auditability; no upstream verification (no signatures to check against).
- Failure handling: warn and continue, never hard-fail. Summary report at end.
- Uncertain-confidence deps (from AST scanner) are never fetched — reported as SKIP.
- `daedalus clean` covers stage cleanup (already removes `~/.cache/daedalus/`).

## Launcher PATH injection (2026-07-17)

- **File**: `stub/src/main.rs` — committed `1c7544a`.
- Added `const BIN_PATHS: &[&str] = &["usr/bin", "bin", "usr/local/bin"]` alongside existing `LD_PATHS`.
- `setup_env()` now injects PATH with rootfs bin dirs prepended (before system PATH), mirroring the LD_LIBRARY_PATH logic exactly.
- Pivot mode: `PATH` = `usr/bin:bin:usr/local/bin` (relative, rootfs IS `/`).
- Non-pivot mode: `PATH` = `{rootfs}/usr/bin:{rootfs}/bin:{rootfs}/usr/local/bin:{existing_PATH}`.
- Bundled binaries take priority over system equivalents — intentional: the app uses the version we packaged.

## Payload encryption (AES-256-GCM)

- **File**: `cli/daedalus/encrypt.py` — `encrypt_payload(plaintext, signing_seed) -> (ciphertext, metadata)`
- **File**: `cli/daedalus/build.py` — `--encrypt` flag, requires `--key` for signing seed (used as AES key via HKDF).
- **File**: `cli/pyproject.toml` — `pip install -e "./cli[encrypt]"` pulls in `cryptography`.
- AES-256-GCM with HKDF key derivation from signing seed. Salt: `daedalus-encrypt-v1`.
- Encrypted payloads produce format v4 footers (`ENCRYPTED_AES_256_GCM` marker). Launcher decrypts after signature verification.
- Signing key = encryption key (whoever can sign can also decrypt). Key rotation planned for future.

## Deno support

- **File**: `cli/daedalus/analyzer/runtime.py` — `_detect_deno()`, `_deno_entry()`
- Detection: looks for `deno.json` / `deno.jsonc` in app directory.
- Entrypoint: reads `tasks.start` / `tasks.dev` / `tasks.default` from deno config, falls back to common names (`main.ts`, `mod.ts`, `index.ts`).
- Embeds deno binary into rootfs at `/usr/bin/deno`.
- **Vendored fallback**: if `deno` is not on PATH, `cross.py:download_vendored_deno()` downloads from GitHub Releases (`deno-{arch}-unknown-linux-gnu.zip`), caches in `~/.cache/daedalus/cross/deno/{arch}/deno`.
- Cross-build for Deno not yet supported (Python only for `--target`).

## daedalus.lock lockfile: Feature D (2026-07-17)

- **File**: `cli/daedalus/analyzer/lockfile.py` — written and tested (not yet committed).
- **Public API**: `detect_or_read_lock(app_dir, redetect, verbose) -> list[DetectedDep] | None`
- **Lockfile**: `daedalus.lock` in app directory — human-readable TOML, never edited by hand.
- **Staleness check**: SHA-256 of Dockerfile content vs `dockerfile_sha256` in lock.
  - No Dockerfile → hash is `"none"`, lock always fresh (useful for pure-Python apps).
  - Hash mismatch → stale, triggers re-detection.
- **Flow**:
  1. `build()` calls `detect_or_read_lock(app_dir, redetect=args.redetect)`
  2. Fresh lock → returns deps, detection skipped.
  3. No lock or stale → runs Dockerfile + AST detection, fetches, writes lock.
  4. `--redetect` flag forces re-detection regardless of lock freshness.
- **Build integration** (`cli/daedalus/build.py`): lockfile check inserted after app_dir resolution, before the daedalus.toml manifest check. Detection deps are recorded but not yet wired into rootfs building (that's a future layer).
- **CLI integration** (`cli/daedalus/cli.py`): `--redetect` flag added to build subcommand.
- **Verified paths**: fresh build (no lock), fresh lock (skip), stale lock (re-detect), --redetect (force), no-Dockerfile app (lock always fresh).

## Docker-compose multi-service warning (2026-07-17)

- **File**: `cli/daedalus/cli.py` — `_parse_compose_services()` + `_warn_multi_service_compose()`.
- **Parser**: regex-based, no YAML dependency (stdlib only). Finds `services:` at indent 0, extracts service names at indent 2, checks for `build:` or `image:` at indent 4.
- **Warning** printed to stderr when >1 service detected: names all services, flags which use `build:` (packageable) vs `image:` (dependencies), states daedalus packages one process.
- **Informational only** — does not block the build, does not affect return code. User sees warning then normal build output.
- **Silent when**: no compose file, single service, unparseable file, or `-q` flag.
- **Verified**: multi-service (build+image), single service, no file, multiple build services, all image services, .yaml extension, comments, quiet mode.

## README rewrite (2026-07-17)

- **File**: `README.md` — full rewrite modeled after Bun's README style.
- **Structure**: centered logo placeholder → title → badges → nav links → "What is x.bin?" → Install → Quick links (4 categories) → Guides (4 categories) → How it works → Example apps → Contributing → License.
- **Logo**: references `logo.png` in repo root — user will create their own.
- **Install**: git clone + `make stub` + `cargo build -p daedalus-cli` — Rust CLI is primary
- **Quick links**: organized by Build, Runtime, Security, CLI — all link to mdbook docs.
- **Guides**: organized by Python, Node.js, Deployment, Security.

---

## What to Do Next

1. **Push** — `0bffc88` is unpushed
2. **Benchmark after optimization** — Run `benchmarks/run-bench.sh` again to measure improvement
3. **Install openssl-dev** for local daedalus-cli testing: `sudo apt install libssl-dev`
4. **Demo recording** — Install `asciinema` + `agg` for YC demo (see demo-yc/)
5. **Optional: rayon for parallel file collection** — tar.rs `collect_entries()` is sequential. Could be parallelized but impact is small vs compression savings.

---

## Roadmap: Features & Limitations

**File**: `ROADMAP.md` — Complete roadmap of features to implement and limitations to address.

### Critical Features (High Priority)
1. Cross-platform support (macOS/Windows)
2. Delta updates (binary patching)
3. Runtime configuration injection
4. Secrets management
5. Persistent storage
6. Observability (logging/metrics/tracing)
7. Sandboxing (seccomp/capabilities/Landlock)
8. Network isolation

### Important Features (Medium Priority)
9. Resource limits (cgroups)
10. Health checks
11. Layer caching
12. Reproducible builds
13. Rollback mechanism
14. Garbage collection
15. WebAssembly support

### Nice to Have (Low Priority)
16. Package registry
17. Desktop integration
18. Auto-update mechanism
19. Multi-container orchestration

See `ROADMAP.md` for detailed implementation plans and timelines.

---

## Database Configuration & Secrets Management

### Problem Statement

When packaging apps with external databases (PostgreSQL, MySQL, MongoDB), secrets (URLs, credentials) must be handled securely. The binary should NOT contain embedded secrets.

### Current Approach (Needs Improvement)

- Secrets are embedded via `--env-file` during build
- Users must rebuild to change secrets
- No runtime configuration mechanism

### Recommended Solution

**Multi-layered configuration system:**

1. **Arguments CLI** (highest priority)
   ```bash
   ./app.ere --db-url "postgresql://user:pass@neon.tech/db"
   ```

2. **Local config file** (fallback)
   ```bash
   # daedalus.toml in same directory as binary
   [database]
   url = "postgresql://..."
   
   [secrets]
   api_key = "xxx"  # Never commit real secrets!
   ```

3. **Environment variables** (standard Unix)
   ```bash
   export DATABASE_URL="postgresql://..."
   ./app.ere
   ```

4. **Interactive prompt** (when TTY available)
   ```bash
   ./app.ere
   Enter DATABASE_URL: ********
   ```

### Implementation Plan

**Phase 1: Stub Enhancement**
- Add `config.rs` module to stub
- Implement multi-source config loading
- Add interactive prompt with masked input
- Support `daedalus.toml` format

**Phase 2: CLI Integration**
- Add `--config` flag to specify config file
- Add `--prompt` flag for interactive mode
- Validate secrets at startup
- Mask secrets in logs/output

**Phase 3: Security Hardening**
- Encrypt sensitive values in config files
- Add secret rotation support
- Audit trail for secret access
- Integration with secret managers (Vault, AWS Secrets)

### File Locations

```
~/.ere/config.toml          # Global config (Unix)
%APPDATA%\daedalus\config.toml   # Global config (Windows)
./daedalus.toml                   # Local config (same dir as binary)
./config.toml                 # Alternative local config
```

### Example: openEMR with External Database

```bash
# Build openEMR (no secrets embedded)
daedalus build openemr/ -o openemr.ere

# Runtime configuration options:
# Option 1: Environment variables
export DATABASE_URL="mysql://openemr:pass@mysql.example.com/openemr"
./openemr.ere

# Option 2: Local config file
echo '[database]\nurl = "mysql://openemr:pass@mysql.example.com/openemr"' > daedalus.toml
./openemr.ere

# Option 3: CLI arguments
./openemr.ere --db-url "mysql://openemr:pass@mysql.example.com/openemr"
```

### Security Best Practices

1. **Never commit real secrets** to git
2. **Use `.env.example`** for local development
3. **Mask secrets** in prompts and output (********)
4. **Validate secrets** at startup
5. **Support rotation** without rebuild
6. **Audit access** to sensitive data

---

## Implementation Status: COMPLETED ✓

### What Was Built

1. **`stub/src/config.rs`** - Configuration module with:
   - `AppConfig` struct with `database` and `secrets` fields
   - `DatabaseConfig` struct for database connection details
   - Multi-layered config loading (CLI → local config → env vars → global config)
   - `prompt_for_secrets()` for interactive masked input
   - Unit tests for all config functionality

2. **Updated `stub/src/main.rs`**:
   - Load config at startup
   - Merge secrets as `DAEDALUS_SECRET_*` environment variables
   - Merge database URL as `DATABASE_URL`

3. **Dependencies added** (`stub/Cargo.toml`):
   - `toml = "0.8"` - config file parsing
   - `dirs = "5"` - standard directories
   - `atty = "0.2"` - TTY detection

### Live Test Results

**Simple PHP App (1.1MB):**
```
DATABASE_URL: mysql://openemr:openemr123@127.0.0.1/openemr
DAEDALUS_SECRET_API_KEY: my-secret-api-key
DAEDALUS_SECRET_DB_PASSWORD: my-db-password
```

**openEMR (92MB):**
```
Built /tmp/openemr-final.ere (91.2MB)
Config loaded correctly from daedalus.toml
```

### Usage Example

```bash
# Build the binary
daedalus build myapp/ -o myapp.ere --embed-interpreter php

# Create config file
cat > myapp.ere.toml << EOF
[database]
url = "mysql://user:pass@host/db"

[secrets]
api_key = "secret-value"
EOF

# Run (secrets loaded from config)
./myapp.ere
```
