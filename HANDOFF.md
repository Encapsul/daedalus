# HANDOFF.md — x.bin project status

## Current state

- **Format**: v5 (SquashFS support)
- **Status**: Phase 2 complete, Phase 3 partially done, CLI compliant with clig.dev
- **Build**: `make stub` + `pip install -e ./cli`
- **Health check**: `xbin doctor` or `make preflight`
- **Branches**: `main` (stable), `dev` (integration), `feat/*` / `fix/*` (features)
- **Release**: `./scripts/release.sh 0.1.0` → CI builds multi-arch binaries → GitHub Release
- **Last commit**: `2158078` — clig.dev audit complete (12 gaps fixed)

---

## LLM Verification Loop (MANDATORY)

**Every time you modify code, before finishing your turn, run this checklist:**

### 1. Did the code change?
If you edited any `.py`, `.rs`, `.toml`, or `.yml` file → continue. Otherwise skip.

### 2. Does it compile / import?
```bash
cd stub && cargo check 2>&1          # Rust
cd cli && python3 -c "import xbin" 2>&1  # Python
```

### 3. Does the app work?
```bash
cd cli && PYTHONPATH=. python3 -m xbin build ../examples/hello-web -o /tmp/test.xbin 2>&1
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
xbin build examples/hello-web -o /tmp/t.xbin
xbin inspect /tmp/t.xbin
xbin keygen --key-dir /tmp/tk
xbin sign /tmp/t.xbin --key /tmp/tk/*.key
xbin verify /tmp/t.xbin --trusted-dir /tmp/tk
```
All must pass. If any fails, your change introduced a regression.

### Rule: never commit without passing this loop.

## Cross-compilation (`--target aarch64`) — 2026-07-18

- **File**: `cli/xbin/cross.py`
- `download_vendored_python(runtime, arch)`: downloads prebuilt Python/Node from `python-build-standalone` (astral-sh) or Node.js official, extracts to `~/.cache/xbin/vendor/{runtime}-{arch}/`
- `pip_download_target(app_dir, requirements, venv_dir, target_arch)`: runs `pip download --only-binary=:all: --platform {manylinux tag} --python-version {ver}` to fetch wheels for target arch
- `_vendored_python_version(vendored_root)`: detects Python version from vendored `lib/pythonX.Y/` directory
- `_unpack_wheel(wheel_path, site_packages_dir)`: extracts wheel `.zip` into site-packages for cross-build
- **File**: `cli/xbin/build.py`
- `build()` rejects non-Python runtimes for cross-build (`node`/`deno` → clear error message)
- `_pip_install_requirements()` accepts `target_arch`: uses `pip_download_target()` when cross-building, falls back to normal pip when native
- `_build_runtime_layer()` / `_build_layers()` / `_build_layers_squashfs()` pass `target_arch` through for cross-build pip
- `_build_runtime_layer()` skips `.so` resolution when using vendored cross-python (no host libs to resolve)

## Dependency checks — 2026-07-18

- **File**: `cli/xbin/doctor.py`
- `xbin doctor` subcommand: checks Python, pip, cargo, rustc, musl target, C compiler, zstd, mksquashfs, node, deno, cryptography, ruff, black, xbin-stub, xbin-crypto
- Each check returns (ok, detail). Required vs optional. Returns exit 1 if any required check fails.
- **File**: `Makefile`
- `make preflight`: quick prerequisite check (python3, pip, cargo, rustc, musl target, cc, zstd). Exit 1 on failure.
- **File**: `cli/pyproject.toml`
- Added `[project.optional-dependencies]`: `encrypt` (cryptography), `python310` (tomli), `dev` (ruff, black), `all`
- **File**: `stub/Cargo.toml`
- Fixed misleading comment: `backhand` (squashfs) requires C compiler via `zstd-sys` — project is not purely pure-Rust

## Doctor --fix (auto-install missing deps) — 2026-07-20

- **File**: `cli/xbin/doctor.py` — `--fix` flag, `--force` / `-f` flag
- `xbin doctor --fix`: attempts to auto-install missing required prerequisites
- `xbin doctor --fix --force`: skips confirmation prompt (for scripts/CI)
- **Fixable checks**: musl target (`rustup target add`), zstd (`apt install`), mksquashfs (`apt install`), cryptography/ruff/black (`pip install`), xbin-stub/xbin-crypto (`make stub`)
- **Non-fixable checks** (manual install required): Python version, pip, cargo, rustc, C compiler, node, deno
- **Safety**: confirms interactively before fixing (unless `--force`); each fix has a timeout; re-verifies after fix; continues on individual failures
- **clig.dev compliance**: `--fix` follows "confirm before dangerous actions" guideline; `--force` for scriptability; exit 0 on full success, 1 on partial/full failure

## Incremental update (`--update` flag) — 2026-07-20

- **File**: `cli/xbin/build.py` — `--update` flag on build subcommand
- **File**: `cli/xbin/assembly.py` — `app_hash` and `rt_deps_hash` params on `build_meta_json()`
- **How it works**:
  - First build: computes `app_hash` (SHA-256 of all app files) and `rt_deps_hash` (SHA-256 of requirements.txt), stores them in the .xbin metadata JSON
  - `xbin build --update`: reads existing .xbin, compares hashes:
    - Same app + same runtime deps → early return ("everything up to date")
    - Same runtime deps, app changed → reuses existing runtime squashfs/zstd blob, only rebuilds app layer
    - Runtime deps changed → full rebuild
- **Helper functions**:
  - `_hash_app_files(app_dir)`: SHA-256 of all files in app_dir (excluding .venv, node_modules, .git, etc.)
  - `_read_existing_xbin(xbin_path, verbose)`: reads footer → meta JSON → extracts runtime layer blob
- **Benefits**: 2-5x faster rebuilds when only app code changes (runtime layer is 12+ MB, app layer is small)
- **Tested**: build → modify app.py → build --update → runtime layer SHA unchanged, app layer SHA changed ✓

## SquashFS support — 2026-07-18

- **File**: `stub/src/format.rs` — format v5, `PAYLOAD_FORMAT_SQUASHFS = 2`
- **File**: `stub/src/main.rs` — squashfs extraction via `squashfs_extract::extract()`, uses backhand crate
- **File**: `stub/src/squashfs_extract.rs` — backhand-based squashfs reader (gzip/lz4/zstd support)
- **File**: `cli/xbin/build.py` — `--squashfs` flag, `mksquashfs` build, tar→squashfs conversion
- **File**: `docs/src/reference/format.md` — v5 format documented
- Metadata `"payload_format": "squashfs"` tells launcher to use squashfs extraction instead of zstd(tar)
- **Note**: SquashFS is a better-compressed layer format (vs zstd+tar). Extraction to disk still happens at startup. Direct mmap without extraction (the real cold-start perf win) is a Phase 3 goal — see "Next steps".

## Ed25519 verification: implemented

- `stub/src/main.rs:70-75` — calls `verify_ed25519()` when `format_version >= 3 && flags & FLAG_SIGNED`.
- `stub/src/main.rs:119-182` — full verification logic: reads sig block at `sig_offset`,
  computes SHA-256(payload ‖ meta_bytes), iterates trusted keys from `$XDG_DATA_HOME/xbin/trusted-keys/`
  (legacy fallback: `~/.xbin/trusted-keys/`), verifies via `ed25519_dalek::Verifier`.
- `stub/Cargo.toml:18` — `ed25519-dalek` with `default-features = false, features = ["alloc"]`.

## Keygen / Sign / Verify CLI: IMPLEMENTED

All implemented in the session of 2026-07-09:

- `stub/Cargo.toml` — added `[[bin]]` target `xbin-crypto`. Also added `rand = "0.8"`.
- `stub/src/bin/xbin-crypto.rs` — three subcommands:
  - `keygen --key-dir <dir>`: generate Ed25519 keypair, write `{fingerprint}.key` (32-byte seed)
    and `{fingerprint}.pub` (32-byte pubkey), print hex fingerprint to stdout.
  - `sign <keyfile>`: read 32-byte SHA-256 hash from stdin, sign, write 64-byte sig to stdout.
    Exit 0 = success, 1 = error.
  - `verify <pubkey>`: read 96 bytes from stdin ([32-byte hash][64-byte sig]), verify.
    Exit 0 = valid, 1 = invalid, 2 = error.
- `cli/xbin/crypto.py` — `find_crypto()` (mirrors `find_stub()`) + thin subprocess wrappers
  for keygen/sign/verify.
- `cli/xbin/keygen.py` — `xbin keygen` CLI (default dir `$XDG_DATA_HOME/xbin/keys`, legacy fallback `~/.xbin/keys`).
- `cli/xbin/sign.py` — `xbin sign <file.xbin>`: reads file with format.py, computes
  SHA-256(payload‖meta), calls crypto.py sign, writes sig_block `[sig_size:u32le][64-byte sig]`
  between metadata and footer, rewrites footer as v3 (format_version=3, flags|=FLAG_SIGNED,
  sig_offset set, footer grown to 92 bytes). In-place modification.
- `cli/xbin/verify.py` — `xbin verify <file.xbin>`: reads v3 footer, iterates trusted keys
  from `$XDG_DATA_HOME/xbin/trusted-keys/` (legacy fallback `~/.xbin/trusted-keys/`, or `--trusted-dir`), calls crypto.py verify for each.
- `cli/xbin/cli.py` — wired up keygen/sign/verify subcommands.
- `Makefile` — `make stub` builds both `xbin-stub` and `xbin-crypto`.
- `.cargo/config.toml` — target-dir is `/tmp/xbin-stub-target` (vfat workaround).
- `find_stub()` and `find_crypto()` now also search `/tmp/xbin-stub-target/`.

## End-to-end test results

```
$ python3 -m xbin build examples/hello-web -o /tmp/hello-web.xbin       → OK (7.1MB)
$ python3 -m xbin keygen --key-dir /tmp/xbin-keys                        → OK, fingerprint printed
$ python3 -m xbin sign /tmp/hello-web.xbin --key <keyfile>               → OK, sig_offset=7117820
$ python3 -m xbin verify /tmp/hello-web.xbin --trusted-dir /tmp/xbin-trusted → OK, exit 0
$ dd if=/dev/urandom of=/tmp/hello-web.xbin bs=1 seek=688788 count=1    → corrupt payload
$ python3 -m xbin verify /tmp/hello-web.xbin --trusted-dir /tmp/xbin-trusted → FAIL, exit 1 (no crash)
```

## Design audit fixes (2026-07-17)

All 14 issues from the design audit have been fixed. Changes verified via Python import tests and pack/unpack round-trip tests.

### Fix #1: inspect.py fingerprint (removed wrong logic)
**File:** `cli/xbin/inspect.py:39-45`
**What changed:** Removed incorrect `hashlib.sha256(sig[:32])` fingerprint computation — it was hashing 32 arbitrary bytes from the Ed25519 signature, not the actual public key. Replaced with actionable guidance: "run 'xbin verify'". Removed unused `hashlib` import. Now uses `fmt.SIG_BLOCK_SIZE` constant instead of hardcoded `68`.

### Fix #2: format.py sig-block constants (eliminated struct duplication)
**Files:** `cli/xbin/format.py`, `cli/xbin/build.py`, `cli/xbin/sign.py`, `cli/xbin/verify.py`, `cli/xbin/inspect.py`
**What changed:** Added `SIG_BLOCK_SIZE = 68`, `SIG_BLOCK_SIZE_FIELD = 64`, `pack_sig_block()`, `unpack_sig_block()` to `format.py`. Updated `sign.py` and `verify.py` to use these helpers (removed `import struct` from both). Updated `inspect.py` to use `fmt.SIG_BLOCK_SIZE`. Removed unused `import struct` from `build.py`. All sig-block logic is now in one place (format.py) matching Rust's `format.rs`.

### Fix #3: cli.py RuntimeError catch (was missing)
**File:** `cli/xbin/cli.py:103`
**What changed:** Added `RuntimeError` to the catch list. `build.py` raises `RuntimeError` at lines 250 and 254 when pip subprocess fails; these were uncaught and would show raw tracebacks to users.

### Fix #4+6: main.rs LD_LIBRARY_PATH + env duplication (DRY refactor)
**File:** `stub/src/main.rs:22-26, 321-386`
**What changed:** Extracted `const LD_PATHS` (lines 24-26), `enter_namespace_if_needed()` (322-327), `setup_env()` (331-366), `make_resolve()` (369-379), and `env_to_cstrings()` (382-386). Both `exec_app()` and `supervise_services()` now call these shared functions instead of duplicating env setup, LD_LIBRARY_PATH construction, and namespace entry logic. LD_LIBRARY_PATH dirs were defined in 4+ places; now in one constant.

### Fix #5: SHA-256 contract documentation
**Files:** `stub/src/format.rs`, `cli/xbin/format.py`
**What changed:** Added cross-reference doc comments in both Rust and Python documenting the integrity hash contract: `SHA-256(payload ‖ metadata_json)`. Also corrected stale wording from `SHA-256(layers ‖ metadata)` to `SHA-256(payload ‖ metadata)` (the old wording was misleading — the hash is over the full payload, not just layers).

### Fix #7: Layer.usize rename (descriptive field name)
**File:** `stub/src/main.rs:59-68`
**What changed:** Renamed `Layer.usize` to `Layer.uncompressed_size` with `#[serde(rename = "usize")]` for JSON compat. Removed `#[allow(dead_code)]` — the field is now descriptively named and actively used.

### Fix #8: _sign_and_write helper (DRY in build.py)
**File:** `cli/xbin/build.py`
**What changed:** Extracted `_sign_and_write()` helper function for the inline-sign-then-write-footer pattern that was duplicated in two places. Removed unused `pub_path` variable.

### Fix #9: _ManifestPlan dead field (removed extra_dirs_host)
**File:** `cli/xbin/build.py`
**What changed:** Removed the unused `extra_dirs_host` field from `_ManifestPlan` dataclass.

### Fix #10: sys.exit → ValueError (proper error flow through cli.py)
**Files:** `cli/xbin/sign.py`, `cli/xbin/verify.py`, `cli/xbin/trust.py`
**What changed:** Converted all `sys.exit(1)` / `sys.exit(0)` to `raise ValueError(...)` (or `return` for the "already trusted" case in trust.py). Errors now flow through `cli.py`'s catch list and get formatted as `[xbin] error: ...` instead of ugly tracebacks or silent exits. Added `try/except FileNotFoundError` around `os.listdir(keys_dir)` in sign.py.

### Fix #11: find_binary extraction (shared utility)
**Files:** `cli/xbin/_util.py` (new), `cli/xbin/build.py`, `cli/xbin/crypto.py`
**What changed:** Created `_util.py` with shared `find_binary()` function (searches PATH + Cargo target dir). Updated `build.py` and `crypto.py` to import from `_util.py` instead of duplicating binary-finding logic. No circular imports — `_util` is a leaf module.

### Fix #12: French → English comments
**File:** `cli/xbin/analyzer/runtime.py:25, 59`, `cli/xbin/cli.py:71`
**What changed:** Translated three French comments to English: "argv relatif au rootfs" → "argv relative to rootfs", "On embarque les site-packages..." → "Embed site-packages...", "remplace le process" → "replaces the process".

### Fix #13: ldd.py inlined to elf.py
**Files:** `cli/xbin/analyzer/ldd.py` (reduced to 2-line re-export), `cli/xbin/build.py`
**What changed:** `ldd.py` was just a one-function wrapper around `elf.shared_libs`. Reduced it to a 2-line backwards-compat re-export from `elf.py`. Updated `build.py` to import from `elf` directly, 3 call sites changed from `ldd.shared_libs` to `elf.shared_libs`.

### Fix #14: __pycache__ cleanup
**What changed:** Removed stale `__pycache__` directories that contained cpython-312 .pyc files alongside cpython-313 ones. Clean builds will regenerate only the correct version.

### Verification
- All Python modules import cleanly with no circular dependencies
- `format.py` pack/unpack sig-block round-trip test passes
- `cargo build --release` + `cargo clippy -- -D warnings` passent clean (0 warnings, 0 errors)
- No test suite exists; `make example` should build a working .xbin to confirm end-to-end

## Clig.dev CLI audit (2026-07-20)

Full audit against https://clig.dev — 12 gaps identified, 11 commits, all fixed.

| # | Gap | Fix | Commit |
|---|-----|-----|--------|
| 1 | Progress on stdout | All `print()` → `file=sys.stderr` (build, clean, lockfile, fetch, cross) | `348e0b4` |
| 2 | `clean --all` no confirmation | Interactive prompt + `-f`/`--force` flag, non-interactive requires `--force` | `fe8aaba` |
| 3 | Silent network fetches | "detecting dependencies..." + "downloading N dependencies..." messages | `9ba1473` |
| 4 | Non-XDG key paths | `$XDG_DATA_HOME/xbin/` with legacy `~/.xbin/` fallback + deprecation warning | `8a2bafe` |
| 5 | No `--version` | `xbin --version` → `xbin 0.1.0` | `f519099` |
| 6 | No machine-readable output | `--json` flag for `inspect` and `doctor` | `9b567f3` |
| 7 | Subcommand abbreviation | Already prevented by argparse (Python 3.12) | N/A |
| 8 | No color support | `_color.py` module: `--no-color`, `NO_COLOR`, `TERM=dumb`, isatty detection | `b36e086` |
| 9 | No isatty checks | `verbose = not args.quiet and sys.stderr.isatty()` — auto-suppress in pipes | `7ae378b` |
| 10 | No help examples | Epilog with 6 usage examples + docs link | `a64dcfd` |
| 11 | No `help` subcommand | `xbin help [command]` via `_SUBPARSERS` dict dispatch | `5652e83` |
| 12 | No exit code docs | 0/1/2 documented in `--help` epilog | `2158078` |

### New files
- `cli/xbin/_color.py` — ANSI color helpers (red, green, yellow, bold) with TTY/NO_COLOR/dumb detection

### Key changes
- `_util.py`: added `keys_dir()` and `trusted_dir()` functions (XDG + legacy fallback)
- `cli.py`: `--no-color` global flag, `--version`, `_SUBPARSERS` dict, `_verbose` with isatty, `help` subcommand, `RawDescriptionHelpFormatter` epilog
- `build.py`: stderr for all progress, network fetch transparency messages, removed dead `_NO_KEYS_MSG`
- `clean.py`: `--force` flag, interactive confirmation for `--all`
- `inspect.py`: refactored with `_collect_inspect_data()` helper, `--json` output
- `doctor.py`: refactored with `_collect_checks()` helper, `--json` output, colored check markers
- `sign.py`, `verify.py`, `trust.py`, `keygen.py`: updated to use `_util.keys_dir()` / `_util.trusted_dir()`
- `lockfile.py`, `fetch.py`, `cross.py`: stderr for all progress messages

## Next steps (future)

### Real-app testing — top 200 GitHub projects (HIGH PRIORITY)
- **Goal**: prove x.bin works on real-world apps, not just toy examples
- **Approach**: test `xbin build` against top 200 GitHub repos (by stars), curate the ones that work as prebuilt downloads
- **Target repos to test** (Python/Node.js focus, apps not libraries):
  - **Python web**: flask (pallets/flask), fastapi (tiangolo/fastapi), django (django/django), sanic (sanic-org/sanic), litestar (litestar-org/litestar)
  - **Python tools**: httpie (httpie/cli), httpx (encode/httpx), thefuck (nvbn/thefuck), borgbackup (borgbackup/borg), pgcli (dbcli/pgcli), mycli (dbcli/mycli), ranger (ranger/ranger), streamlink (streamlink/streamlink), you-get (soimort/you-get), yt-dlp (yt-dlp/yt-dlp), tldr (tldr-pages/tldr)
  - **Python data**: jupyter (jupyter/jupyter), numpy (numpy/numpy), pandas (pandas-dev/pandas), matplotlib (matplotlib/matplotlib), scikit-learn (scikit-learn/scikit-learn), polars (pola-rs/polars)
  - **Python infra**: ansible (ansible/ansible), fabric (fabric/fabric), invoke (pyinvoke/invoke), salt (saltstack/salt)
  - **Node.js**: express (expressjs/express), next.js (vercel/next.js), n8n (n8n-io/n8n), Ghost (TryGhost/Ghost), PM2 (Unitech/pm2), homebridge (homebridge/homebridge), mosca (moscajs/mosca)
  - **Go (future)**: caddy (caddyserver/caddy), traefik (traefik/traefik), hugo (gohugoio/hugo), lazygit (jesseduffield/lazygit)
- **Process**: for each repo, `git clone` → `xbin build` → test run → document what works/breaks
- **Distribution**: working builds become official downloads on the website (`xbin.sh/downloads`)
- **Why**: marketing proof point ("we can build Flask, FastAPI, yt-dlp, n8n…"), real-world bug discovery, performance benchmarks
- **File**: track results in `TESTED_APPS.md` at repo root (pass/fail, size, notes)

### Install script + upgrade command
- `scripts/install.sh` — curl-pipe-bash installer (like bun.sh, get.wasmer.io)
- `xbin upgrade` — self-update command
- See release strategy section in conversation notes

### Remaining features
- Cross-build aarch64 stub locally: requires `rustup target add aarch64-unknown-linux-musl` + cross-linker. CI handles this automatically via GitHub Actions runners.
- `xbin sign` with automatic key lookup in `$XDG_DATA_HOME/xbin/keys/` (without `--key`).
- `xbin scan` — scan installed xbin packages for updates/vulnerabilities
- `squashfs + mmap` direct read (kernel mount, Linux 5.12+, no extraction needed) — the real cold-start perf win beyond just better compression.
- LRU cache cleanup (evict beyond threshold)
- Cold/warm start < 100 ms end-to-end
- Distribution / discovery (lightweight registry)
- Run full end-to-end build+sign+verify cycle for aarch64 once stub is compiled locally
- GitHub Actions official action (`action-xbin/build`) — for CI/CD workflows

## Seccomp BPF denylist (2026-07-17)

- **File**: `stub/src/main.rs` — `install_seccomp_denylist()`.
- **Approach**: Denylist (not allowlist). Conservative: ~14 syscalls blocked, everything else allowed.
- **Rationale**: Python/Node.js use 150+ distinct syscalls. An allowlist would break apps unpredictably. A denylist of clearly dangerous syscalls is sufficient — namespace isolation handles the rest.
- **Blocked syscalls**: ptrace, mount, umount2, pivot_root, reboot, kexec_load, kexec_file_load, init_module, finit_module, delete_module, swapon, swapoff, sethostname, setdomainname, acct, nfsservctl.
- **Hook point**: After `pivot_root_into()` in both `exec_app()` (single-service) and `supervise_services()` (multi-service). Filter applies before `execve()` — child processes inherit it.
- **Graceful degradation**: If `prctl(PR_SET_SECCOMP)` fails, prints `[xbin] warning` to stderr and continues. Never blocks execution.
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
- **build.py**: extracted 10+ helpers (`_resolve_app_path`, `_resolve_service_binary`, `_collect_service_bins`, `_build_meta_json`, `_assemble_xbin`, `_build_layers`, `_copy_service_layers`, `_copy_app_files`, `_install_manifest_pip`, `_build_service_metadata`) — `_build_manifest` 171→35, `build` 138→40 lines. Committed `c7e402e`.
- **trust.py**: removed unused `import os` (ruff F401). Committed `4d938d3`.
- **crypto.py**: Black formatting. Committed `4d938d3`.
- **cli.py**: Black formatting. Committed `4d938d3`.
- **runtime.py**: extracted `_detect_python()`, `_detect_node()` — `detect()` 68→25 lines. Committed `c92e114`.
- **elf.py**: removed unused `import os` (F401), removed dead `sub_dirs` assignment (F841). Bug fix: `_resolve_recursive` now carries per-library search_dirs in queue `(name, dirs)` so each library resolves its own deps via DT_RUNPATH. Committed `c92e114` + `917b059`.
- **format.rs**: removed WHAT-style numbered comments. Committed `e56146e`.
- **main.rs**: added SAFETY comments to all 8 unsafe blocks, split `supervise_services` (104→15 lines) into `fork_services`(45), `wait_for_health`(14), `wait_for_children`(40). Committed `fae9f56`.

## Dockerfile dependency analyzer: Feature A (2026-07-17)

- **File**: `cli/xbin/analyzer/dockerfile.py` — committed `ebea282`.
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

- **File**: `cli/xbin/analyzer/python_ast.py` — committed `638b868`.
- **Public API**: `detect_from_python_source(app_dir: Path) -> list[DetectedDep]`
- **Merge utility**: `merge_deps(dockerfile_deps, ast_deps) -> list[DetectedDep]`
- Walks `ast.Call` nodes for `subprocess.run/Popen/call/check_call/check_output/getoutput/getstatusoutput` and `os.system/os.popen`.
- Extracts literal binary names from string args (`"ffmpeg -i ..."`) and list args (`["ffmpeg", ...]`).
- Variables/expressions flagged as `confidence="uncertain"`.
- Skips builtins (python, node, bash, sh, etc.).
- Dedup merge: Dockerfile wins over AST for same binary name.
- Python only for now. Node.js gap documented (requires JS parser).

## Feature C: dependency fetcher into staging (2026-07-17)

- **File**: `cli/xbin/analyzer/fetch.py` — committed next.
- **Public API**: `fetch_deps(deps, verbose) -> (stage_dir, list[FetchResult])`
- **FetchResult** dataclass: `dep`, `ok`, `error`, `sha256`.
- Staging directory: `~/.cache/xbin/stage/{SHA-256 of sorted dep list}/` with subdirs per kind.
- Fetchers per dependency type:
  - **pip**: `pip download --no-deps --dest {stage}/pip/` (never installs globally)
  - **npm**: `npm install --prefix {stage}/npm/ --save=false` (never touches global node_modules)
  - **apt**: `apt-get download` + `dpkg-deb -x` into staging (never `apt-get install`)
  - **apk**: `apk fetch --simulate --stdout` + write/extract .apk into staging
  - **external**: `urllib.request.urlretrieve` + extract archive into staging
- Checksum handling: SHA-256 recorded in `manifest.json` for auditability; no upstream verification (no signatures to check against).
- Failure handling: warn and continue, never hard-fail. Summary report at end.
- Uncertain-confidence deps (from AST scanner) are never fetched — reported as SKIP.
- `xbin clean` covers stage cleanup (already removes `~/.cache/xbin/`).

## Launcher PATH injection (2026-07-17)

- **File**: `stub/src/main.rs` — committed `1c7544a`.
- Added `const BIN_PATHS: &[&str] = &["usr/bin", "bin", "usr/local/bin"]` alongside existing `LD_PATHS`.
- `setup_env()` now injects PATH with rootfs bin dirs prepended (before system PATH), mirroring the LD_LIBRARY_PATH logic exactly.
- Pivot mode: `PATH` = `usr/bin:bin:usr/local/bin` (relative, rootfs IS `/`).
- Non-pivot mode: `PATH` = `{rootfs}/usr/bin:{rootfs}/bin:{rootfs}/usr/local/bin:{existing_PATH}`.
- Bundled binaries take priority over system equivalents — intentional: the app uses the version we packaged.

## Payload encryption (AES-256-GCM)

- **File**: `cli/xbin/encrypt.py` — `encrypt_payload(plaintext, signing_seed) -> (ciphertext, metadata)`
- **File**: `cli/xbin/build.py` — `--encrypt` flag, requires `--key` for signing seed (used as AES key via HKDF).
- **File**: `cli/pyproject.toml` — `pip install -e "./cli[encrypt]"` pulls in `cryptography`.
- AES-256-GCM with HKDF key derivation from signing seed. Salt: `xbin-encrypt-v1`.
- Encrypted payloads produce format v4 footers (`ENCRYPTED_AES_256_GCM` marker). Launcher decrypts after signature verification.
- Signing key = encryption key (whoever can sign can also decrypt). Key rotation planned for future.

## Deno support

- **File**: `cli/xbin/analyzer/runtime.py` — `_detect_deno()`, `_deno_entry()`
- Detection: looks for `deno.json` / `deno.jsonc` in app directory.
- Entrypoint: reads `tasks.start` / `tasks.dev` / `tasks.default` from deno config, falls back to common names (`main.ts`, `mod.ts`, `index.ts`).
- Embeds deno binary into rootfs at `/usr/bin/deno`.
- **Vendored fallback**: if `deno` is not on PATH, `cross.py:download_vendored_deno()` downloads from GitHub Releases (`deno-{arch}-unknown-linux-gnu.zip`), caches in `~/.cache/xbin/cross/deno/{arch}/deno`.
- Cross-build for Deno not yet supported (Python only for `--target`).

## xbin.lock lockfile: Feature D (2026-07-17)

- **File**: `cli/xbin/analyzer/lockfile.py` — written and tested (not yet committed).
- **Public API**: `detect_or_read_lock(app_dir, redetect, verbose) -> list[DetectedDep] | None`
- **Lockfile**: `xbin.lock` in app directory — human-readable TOML, never edited by hand.
- **Staleness check**: SHA-256 of Dockerfile content vs `dockerfile_sha256` in lock.
  - No Dockerfile → hash is `"none"`, lock always fresh (useful for pure-Python apps).
  - Hash mismatch → stale, triggers re-detection.
- **Flow**:
  1. `build()` calls `detect_or_read_lock(app_dir, redetect=args.redetect)`
  2. Fresh lock → returns deps, detection skipped.
  3. No lock or stale → runs Dockerfile + AST detection, fetches, writes lock.
  4. `--redetect` flag forces re-detection regardless of lock freshness.
- **Build integration** (`cli/xbin/build.py`): lockfile check inserted after app_dir resolution, before the xbin.toml manifest check. Detection deps are recorded but not yet wired into rootfs building (that's a future layer).
- **CLI integration** (`cli/xbin/cli.py`): `--redetect` flag added to build subcommand.
- **Verified paths**: fresh build (no lock), fresh lock (skip), stale lock (re-detect), --redetect (force), no-Dockerfile app (lock always fresh).

## Docker-compose multi-service warning (2026-07-17)

- **File**: `cli/xbin/cli.py` — `_parse_compose_services()` + `_warn_multi_service_compose()`.
- **Parser**: regex-based, no YAML dependency (stdlib only). Finds `services:` at indent 0, extracts service names at indent 2, checks for `build:` or `image:` at indent 4.
- **Warning** printed to stderr when >1 service detected: names all services, flags which use `build:` (packageable) vs `image:` (dependencies), states xbin packages one process.
- **Informational only** — does not block the build, does not affect return code. User sees warning then normal build output.
- **Silent when**: no compose file, single service, unparseable file, or `-q` flag.
- **Verified**: multi-service (build+image), single service, no file, multiple build services, all image services, .yaml extension, comments, quiet mode.

## README rewrite (2026-07-17)

- **File**: `README.md` — full rewrite modeled after Bun's README style.
- **Structure**: centered logo placeholder → title → badges → nav links → "What is x.bin?" → Install → Quick links (4 categories) → Guides (4 categories) → How it works → Example apps → Contributing → License.
- **Logo**: references `logo.png` in repo root — user will create their own.
- **Install**: git clone + `make stub` + `pip install -e ./cli` — no curl installer, no brew.
- **Quick links**: organized by Build, Runtime, Security, CLI — all link to mdbook docs.
- **Guides**: organized by Python, Node.js, Deployment, Security.
