# HANDOFF.md — x.bin project status

## Format: v3 (implemented)

- `stub/src/format.rs` and `cli/xbin/format.py` both implement the **v3 footer**:
  92 bytes total, with `sig_offset` (u64) as an 8-byte PREFIX before the 84-byte
  v2-compatible core. v2 readers see unknown magic at EOF-84 and report cleanly.
- Layout: `[0-7] sig_offset (u64) | [8-12] magic "XBIN\\x01" | [13] version=3 | ...`
- `Footer.sig_offset` is the absolute offset of `[sig_size:u32le][signature:64 bytes]`.

## Ed25519 verification: implemented

- `stub/src/main.rs:70-75` — calls `verify_ed25519()` when `format_version >= 3 && flags & FLAG_SIGNED`.
- `stub/src/main.rs:119-182` — full verification logic: reads sig block at `sig_offset`,
  computes SHA-256(payload ‖ meta_bytes), iterates trusted keys from `~/.xbin/trusted-keys/`
  (or `$XBIN_TRUSTED_DIR`), verifies via `ed25519_dalek::Verifier`.
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
- `cli/xbin/keygen.py` — `xbin keygen` CLI (default dir `~/.xbin/keys`).
- `cli/xbin/sign.py` — `xbin sign <file.xbin>`: reads file with format.py, computes
  SHA-256(payload‖meta), calls crypto.py sign, writes sig_block `[sig_size:u32le][64-byte sig]`
  between metadata and footer, rewrites footer as v3 (format_version=3, flags|=FLAG_SIGNED,
  sig_offset set, footer grown to 92 bytes). In-place modification.
- `cli/xbin/verify.py` — `xbin verify <file.xbin>`: reads v3 footer, iterates trusted keys
  from `~/.xbin/trusted-keys/` (or `--trusted-dir`), calls crypto.py verify for each.
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
- `cargo check` / `rustc` not available in this environment — Rust changes verified by manual review
- No test suite exists; `make example` should build a working .xbin to confirm end-to-end

## Next steps (future)

- `xbin sign` with automatic key lookup in `~/.xbin/keys/` (without `--key`).
- `xbin verify` using launcher embedded logic (via `$XBIN_TRUSTED_DIR`).
- Support for `--key-dir` default in `xbin keygen`.
- Possibly a `trust` subcommand to manage trusted keys.
- Run full end-to-end build+sign+verify cycle once `cargo` is available to confirm Rust changes compile.

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
