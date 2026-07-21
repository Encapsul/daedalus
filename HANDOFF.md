# HANDOFF.md — x.bin project status

## Current state

- **Format**: v5 (SquashFS support)
- **Status**: Phase 1 complete, Phase 2 pending, CLI compliant with clig.dev
- **Build**: `make stub` + `pip install -e ./cli`
- **Health check**: `xbin doctor` or `make preflight`
- **Branches**: `main` (stable), `dev` (integration), `feat/*` / `fix/*` (features)
- **Release**: create release on GitHub UI → `on: release: types: [published]` triggers workflow → builds 4 platforms → uploads binaries + SHASUMS256.txt via `gh release upload --clobber`
- **Runtimes**: Python, Node.js, Deno, Java, Ruby, .NET/C#, Go, PHP, Perl, Binary, Hugo (11 total)
- **Framework support**: Next.js, Nuxt, Astro, Remix, SvelteKit, Express, Fastify, Hono, Django, FastAPI, Flask, Laravel, Symfony (auto-detected)
- **Rust core**: `xbin-core` crate — Phase 1 complete, stub uses shared library. Phase 2 (PyO3 bindings) started: format wired via `_format.py` wrapper
- **Tests**: 234 Python + 26 Rust = 260 total (0 failures)
- **Hugo**: runtime rewritten — builds at detect time, serves static files via python3 http.server
- **`.env` baking**: implemented — `--env-file` flag, secret detection, bake into app layer
- **Version metadata**: `--version-info`, `--author`, `--description`, `--license` flags
- **Persistent storage**: `--persist` flag, `XBIN_PERSIST_DIR` env var
- **Data files**: `--include PATH` flag (repeatable) to embed files/dirs in binary
- **Tree-shaking**: `--tree-shake` removes unused node_modules packages
- **Minification**: `--minify` shrinks JS/TS/CSS files before packaging
- **Framework auto-detect**: Express, Fastify, Hono, Remix, SvelteKit, FastAPI, Flask detected from deps/source
- **Health checks**: `--health-port` for /healthz, /readyz, /status endpoints
- **OpenTelemetry**: `--otel-endpoint` for auto-instrumentation, OTLP export
- **Cron/scheduled tasks**: `--cron NAME:SCHEDULE` for built-in periodic tasks
- **Last updated**: 2026-07-21

---

## LLM Verification Loop (MANDATORY)

**Every time you modify code, before finishing your turn, run this checklist:**

### ⚠️ RULE: NEVER commit without running ALL of these first:
```bash
cd cli && python3 -m ruff check xbin/     # lint
cd cli && python3 -m black --check xbin/  # format
cd cli && python3 -m pytest tests/ -q     # tests
```

### 1. Did the code change?
If you edited any `.py`, `.rs`, `.toml`, or `.yml` file → continue. Otherwise skip.

### 2. Does it compile / import?
```bash
cd stub && cargo check 2>&1                                  # Rust
cd cli && python3 -c "import xbin" 2>&1                      # Python import
cd cli && python3 -m ruff check xbin/ 2>&1                   # Lint (must pass)
cd cli && python3 -m black --check xbin/ 2>&1                # Format (must pass)
cd cli && python3 -m pytest tests/ -q 2>&1                   # Tests (must pass)
```
**ALL FOUR MUST PASS before finishing your turn. No exceptions.**

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

## xbin scan (discover .xbin files) — 2026-07-20

- **File**: `cli/xbin/scan.py` — `scan(paths, json_output)` function
- **File**: `cli/xbin/cli.py` — `scan` subparser with `paths` (nargs="*", default=["."]) and `--json`
- **File**: `cli/xbin/_util.py` — `cache_dir()` moved here from `clean.py` (shared by scan + clean)
- **File**: `cli/xbin/clean.py` — imports `cache_dir` from `_util` instead of defining locally
- **How it works**:
  - Recursively finds `.xbin` files by extension + footer magic (`0xBEEFCAFE`)
  - Reads metadata from each file (reuses `format.read_footer()`)
  - Displays table: FILE, NAME, RUNTIME, ARCH, SIGNED, CREATED
  - Shows cache stats (entries + total size from `~/.cache/xbin/`)
  - `--json` outputs structured JSON with all metadata fields
- **Exit codes**: 0 if files found, 1 if none found
- **Tested**: scan /tmp/ (found 4 files), scan --json, scan /nonexistent (exit 1), scan examples/ (exit 1) ✓

## New runtimes + release fix — 2026-07-20

### New runtimes: Go, PHP, Perl

- **File**: `cli/xbin/runtimes/go.py` — Go runtime
  - Detection: `go.mod` in project root
  - Builds static binary via `go build`, embeds into .xbin
  - Cross-compilation supported (GOOS/GOARCH)
- **File**: `cli/xbin/runtimes/php.py` — PHP runtime
  - Detection: `composer.json` in project root
  - Framework detection: Laravel (artisan), Symfony (symfony.lock), WordPress (wp-config.php)
  - Entry point: public/index.php, index.php, bin/console, artisan
- **File**: `cli/xbin/runtimes/perl.py` — Perl runtime
  - Detection: `Makefile.PL` or `cpanfile` in project root
  - Entry point: app.pl, bin/app, main.pl, server.pl, app.psgi
- **File**: `cli/xbin/runtimes/__init__.py` — updated registry
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

- **Bug**: `release.yml` only packaged `xbin-stub` + `xbin-crypto` (Rust binaries), NOT the Python CLI (`xbin`). Users could not run `xbin` after installing from a release.
- **File**: `.github/workflows/release.yml` — restructured:
  - Packages full CLI bundle: Python package + Rust stubs + wrapper script
  - Naming: `xbin-{os}-{arch}.tar.gz` (Bun/Wasmer pattern, no version in dir name)
  - SHA-256 checksums included
  - Release notes with changelog, install instructions, checksums section
- **File**: `scripts/install.sh` — updated to match new structure:
  - Expects `xbin-{platform}/bin/xbin` wrapper script
  - Handles both `sha256sum` (Linux) and `shasum` (macOS)
  - Installs Python CLI lib to `{INSTALL_DIR}/../lib/xbin/python/`
  - Updates wrapper script with correct lib path
- **Architecture**: wrapper script sets `PYTHONPATH` to find bundled Python CLI, then execs `python3 -m xbin`

### Documentation

- **File**: `README.md` — added Go, PHP, Perl to runtime table, guides, and quick links
- **File**: `docs/src/introduction.md` — updated runtime list
- **File**: `docs/src/SUMMARY.md` — added Go, PHP, Perl guide entries
- **File**: `docs/src/guides/go.md` — new guide page
- **File**: `docs/src/guides/php.md` — new guide page
- **File**: `docs/src/guides/perl.md` — new guide page

## Framework-specific detection — 2026-07-20

### Enhanced runtime detectors

**Node.js** (`cli/xbin/runtimes/node.py`):
- Framework detection: Next.js (`next.config.js/mjs/ts`), Nuxt (`nuxt.config.ts/js/mjs`), Astro (`astro.config.mjs/ts`)
- Reads `scripts.start` from `package.json` as fallback entrypoint
- Next.js: entrypoint = `next start`
- Nuxt: entrypoint = `nuxt start`
- Astro SSR: entrypoint = `dist/server/entry.mjs` (after build) or `astro start`
- Generic: `main` field → `index.js`/`server.js`/`app.js`

**Python** (`cli/xbin/runtimes/python.py`):
- Django detection: `manage.py` + `wsgi.py`/`asgi.py` in subdirectory
- Auto-finds gunicorn (WSGI) or uvicorn (ASGI) on PATH
- Fallback: `manage.py runserver 0.0.0.0:8000`
- Generic: `app.py`/`main.py`/`__main__.py`/`server.py`

**PHP** (`cli/xbin/runtimes/php.py`):
- Laravel: `php artisan serve --host=0.0.0.0 --port=8000` (was just `artisan` which prints help)
- Symfony: `php bin/console server:run 0.0.0.0:8000`
- WordPress: `php -S 0.0.0.0:8080 -t /app` (PHP built-in server)
- Generic: `php -S 0.0.0.0:8000 -t /app/public`

**Hugo** (`cli/xbin/runtimes/hugo.py`) — REWRITTEN RUNTIME:
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

- **File**: `cli/xbin/dotenv.py` — NEW module
  - `parse_dotenv(env_file)`: parses KEY=value format (export prefix, quotes, comments, empty lines, values with `=`)
  - `detect_secret_keys(env)`: warns on `PASSWORD`, `SECRET`, `TOKEN`, `API_KEY`, `PRIVATE_KEY`, `CREDENTIALS` patterns
  - `load_dotenv(app_dir, env_file, verbose)`: resolves path relative to app_dir, parses, warns on secrets
- **File**: `cli/xbin/cli.py` — `--env-file FILE` flag on build subcommand
- **File**: `cli/xbin/build.py` — `env_file` param on `build()`, resolves to `env_file_path`, threads through `build_app_layer()` + `build_layers()`
- **File**: `cli/xbin/layers.py` — `env_file_path` param on `build_app_layer()` and `build_layers()`:
  - Copies external `.env` file into app layer as `.env`
  - If `plan.env` is set (from xbin.toml), writes a `.env` file with those key-value pairs
- **Flow**: `--env-file .env` → parse → merge into `plan.env` (set as real env vars by launcher) + copy file into app layer
- **Test file**: `cli/tests/test_dotenv.py` — 15 tests (parse_dotenv, detect_secret_keys, load_dotenv)
- **Status**: implemented, wired through build pipeline, tests passing

## Version metadata — 2026-07-20

- **File**: `cli/xbin/cli.py` — `--version-info`, `--author`, `--description`, `--license` flags on build subcommand
- **File**: `cli/xbin/build.py` — passes version/author/description/license to `build_meta_json()`
- **File**: `cli/xbin/assembly.py` — `build_meta_json()` accepts and includes version/author/description/license in metadata JSON
- **File**: `cli/xbin/inspect.py` — displays version/author/description/license when present
- **Flow**: `--version-info 1.0 --author "John"` → stored in `.xbin` metadata JSON → displayed by `xbin inspect`
- **Test file**: `cli/tests/test_version_metadata.py` — 6 tests
- **Status**: implemented, committed `961c526`

## Persistent storage — 2026-07-20

- **File**: `cli/xbin/persistent.py` — NEW module
  - `get_persist_dir(app_name)` → `~/.local/share/xbin/{app-name}/` (XDG compliant)
  - `ensure_persist_dir()` creates directory
  - `get_persist_env()` returns `{"XBIN_PERSIST_DIR": "<path>"}`
- **File**: `cli/xbin/cli.py` — `--persist` flag on build subcommand
- **File**: `cli/xbin/build.py` — injects `XBIN_PERSIST_DIR` into `plan.env` when `--persist` is set
- **Flow**: `--persist` → sets `XBIN_PERSIST_DIR` env var → app reads it for persistent data
- **Test file**: `cli/tests/test_persistent.py` — 7 tests
- **Status**: implemented, committed `9872d53`

## Data files (--include) — 2026-07-20

- **File**: `cli/xbin/cli.py` — `--include PATH` flag (repeatable, `action="append"`)
- **File**: `cli/xbin/build.py` — resolves include paths relative to app_dir, validates existence
- **File**: `cli/xbin/layers.py` — `build_app_layer()` and `build_layers()` accept `include_paths` param
- **Flow**: `--include data/config.json --include templates/` → copies files/dirs into app layer
- **Test file**: `cli/tests/test_include.py` — 6 tests (file, dir, multiple, none, overwrite, symlink)
- **Status**: implemented, committed `6ea54a5`

## Tree-shaking — 2026-07-20

- **File**: `cli/xbin/treeshake.py` — NEW module
  - `detect_used_packages(app_dir)` → scans JS/TS source for require() and import statements
  - `prune_node_modules(app_dir)` → removes unused top-level packages from node_modules
- **File**: `cli/xbin/cli.py` — `--tree-shake` flag on build subcommand
- **File**: `cli/xbin/build.py` — runs `prune_node_modules()` before layer building
- **Flow**: `--tree-shake` → scan source → resolve used packages → remove unused from node_modules
- **Test file**: `cli/tests/test_treeshake.py` — 10 tests (detect, prune, scoped packages)
- **Status**: implemented, committed `0a1c5a9`

## Minification — 2026-07-20

- **File**: `cli/xbin/minify.py` — NEW module
  - `minify_app_dir(app_dir)` → minifies JS/TS (via terser) and CSS (built-in stripper)
- **File**: `cli/xbin/cli.py` — `--minify` flag on build subcommand
- **File**: `cli/xbin/build.py` — runs `minify_app_dir()` before layer building
- **Flow**: `--minify` → scan app dir → minify JS/TS via terser, CSS via whitespace stripping
- **Test file**: `cli/tests/test_minify.py` — 7 tests (CSS, JS, skip node_modules, no files)
- **Status**: implemented, committed `74d2011`

## Framework auto-detect (enhanced) — 2026-07-20

- **File**: `cli/xbin/runtimes/node.py` — enhanced `_detect_framework()`:
  - Config-file detection: Next.js, Nuxt, Astro, Remix, SvelteKit
  - Dependency-based detection: Express, Fastify, Hono (from package.json)
  - Entrypoint builders for Remix (`remix-serve build`), SvelteKit (`svelte-kit dev`), Express/Fastify (`node entry.js`), Hono (`node src/index.ts`)
- **File**: `cli/xbin/runtimes/python.py` — added FastAPI and Flask detection:
  - `_detect_fastapi()` — scans source for `from fastapi import`, auto-selects uvicorn
  - `_detect_flask()` — scans source for `from flask import`
  - `_build_python_plan()` — shared builder for detected frameworks
- **Test file**: `cli/tests/test_framework_detect.py` — 15 tests (10 Node, 5 Python)
- **Status**: implemented, committed `c82c0d2`

## Health checks — 2026-07-20

- **File**: `cli/xbin/health.py` — NEW module
  - `HealthState` class: mark_ready(), mark_not_ready(), uptime, version, extra fields
  - HTTP server: `/healthz` (liveness, always 200), `/readyz` (readiness), `/status` (JSON)
  - `start_health_server(port)` — background thread, daemon
  - `get_health_state()` — global singleton for app code
- **File**: `cli/xbin/cli.py` — `--health-port PORT` flag on build subcommand
- **File**: `cli/xbin/build.py` — injects `XBIN_HEALTH_PORT` into plan.env
- **Flow**: `--health-port 8081` → launcher starts HTTP server → app marks ready via `xbin.health.mark_ready()`
- **Test file**: `cli/tests/test_health.py` — 12 tests (state, server endpoints, disabled)
- **Status**: implemented, committed `0da1980`

## OpenTelemetry — 2026-07-20

- **File**: `cli/xbin/otel.py` — NEW module
  - `build_otel_env()` — builds OTEL_SERVICE_NAME, OTEL_RESOURCE_ATTRIBUTES, OTEL_EXPORTER_OTLP_ENDPOINT, etc.
  - `get_otel_config()` — reads current OTel config from environment
  - `format_resource_attributes()` — parses "key=value,key2=value2" string
- **File**: `cli/xbin/cli.py` — `--otel-endpoint URL` and `--otel-protocol` flags
- **File**: `cli/xbin/build.py` — injects OTel env vars into plan.env
- **Flow**: `--otel-endpoint http://localhost:4317` → OTEL_* env vars set → app auto-instruments
- **Test file**: `cli/tests/test_otel.py` — 15 tests (env building, config reading, resource attrs)
- **Status**: implemented, committed `71b2c28`

## Cron/scheduled tasks — 2026-07-20

- **File**: `cli/xbin/cron.py` — NEW module
  - `CronScheduler` — background thread, tick-based, @every/@hourly/@daily/@weekly + cron-style
  - `Task` dataclass — name, schedule, func, error tracking
  - `get_scheduler()` — global singleton
  - `build_cron_env()` — XBIN_CRON_ENABLED + XBIN_CRON_TASKS env vars
- **File**: `cli/xbin/cli.py` — `--cron NAME:SCHEDULE` flag (repeatable)
- **File**: `cli/xbin/build.py` — parses cron tasks, injects env vars
- **Flow**: `--cron cleanup:'*/5 * * * *'` → XBIN_CRON_TASKS JSON → app registers tasks
- **Test file**: `cli/tests/test_cron.py` — 22 tests (schedule parsing, scheduler, error handling)
- **Status**: implemented, committed `465a3c9`

## Package manager support — 2026-07-20

### New module: `cli/xbin/pkgmgr.py`

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

**File**: `cli/xbin/build.py` — added `no_install` param, pkgmgr integration
**File**: `cli/xbin/cli.py` — added `--no-install` flag to build subcommand

### Unit tests

- **File**: `cli/tests/test_pkgmgr.py` — 17 tests
- **Total**: 38 tests, all passing

## Rust migration plan — 2026-07-20

### Strategy: layered migration, not rewrite

```
Phase 1 (NOW)           Phase 2              Phase 3
────────────────        ────────────         ──────────
Python CLI +            Rust core lib        Full Rust binary
Rust launcher           (xbin-core)          (xbin)
                        
build.py                xbin-core crate      xbin (single binary)
  → calls Rust lib       ├── format          no more Python
                         ├── compress
                         ├── detect
                         ├── pkgmgr
                         └── crypto
Python remains for:
  → AST scanning (analyzer/)
  → Dockerfile parsing
  → xbin.toml manifest
```

### Phase 1: xbin-core crate — ✅ COMPLETE

**Directory**: `xbin-core/` at repo root

**Modules created**:
- `format.rs` — .xbin footer parsing (v1-v5), 15 unit tests
- `compress.rs` — zstd compress/decompress, 1 unit test
- `detect.rs` — runtime detection by file markers, 5 unit tests
- `pkgmgr.rs` — package manager detection by lock files, 5 unit tests

**Dependencies**: sha2, serde, serde_json, ruzstd, zstd (C bindings)

**Tests**: 26 Rust unit tests, all passing, clippy clean

**Wiring**: stub uses `xbin-core` as path dependency — `stub/src/format.rs` deleted, format parsing shared via `use xbin_core::format`.

**Next steps**: Phase 2 — xbin-core becomes full core lib, Python becomes thin wrapper

### Phase 2: Rust core lib — IN PROGRESS

**Goal**: `xbin-core` handles all build logic. Python becomes thin wrapper.

**PyO3 bindings** (`xbin-core/src/python.rs`):
- Exposed: format (Footer, constants, read_footer, read_at), compress (compress/decompress), detect (runtime + all individual runtimes), pkgmgr (detect + install_cmd)
- Built via `maturin build --release`, installed as `xbin_core` Python package
- `cli/xbin/_format.py` wrapper: delegates to `xbin_core` when available, falls back to pure Python

**Format wiring** (DONE):
- 7 files switched from `from . import format as fmt` to `from . import _format as fmt`:
  - build.py, scan.py, assembly.py, selftest.py, verify.py, inspect.py, sign.py
- `_format.py` provides identical API, transparently uses Rust when available

**Remaining to wire**:
- `compress`: currently uses Python zstd in `layers.py`, no separate `compress.py` module
- `detect`: currently in `analyzer/runtime.py`, calls Python runtime detectors
- `pkgmgr`: currently in `pkgmgr.py`, pure Python detection

**Rust modules to add** (future):
- `build.rs` — build orchestration (currently `cli/xbin/build.py`)
- `layers.rs` — layer construction (currently `cli/xbin/layers.py`)
- `assembly.rs` — .xbin assembly (currently `cli/xbin/assembly.py`)
- `sign.rs` — Ed25519 signing (currently `cli/xbin/sign.py`)
- `verify.rs` — signature verification (currently `cli/xbin/verify.py`)

**Python modules that stay** (hard to port):
- `analyzer/python_ast.py` — AST scanning (needs tree-sitter or syn)
- `analyzer/dockerfile.py` — Dockerfile parsing (regex-heavy, low value to port)
- `analyzer/elf.py` — ELF shared lib resolution (needs `goblin` crate)
- `manifest.py` — xbin.toml parsing (serde can handle this)

### Phase 3: Full Rust CLI

**Goal**: Single binary, no Python dependency.

**CLI framework**: `clap` (already used in xbin-crypto)
**Config**: `clap` derive macros
**Testing**: `insta` for snapshot tests

**What changes**:
- `xbin build ./myapp` = Rust binary, no Python
- `xbin doctor` = Rust binary
- `xbin inspect` = Rust binary
- `xbin sign`/`verify`/`keygen` = already Rust
- Distribution: single binary, no `pip install` needed

### Why this order

1. **Format + compress** — highest value, already proven in stub
2. **Detect + pkgmgr** — pure logic, no external deps, easy to test
3. **Build orchestration** — most complex, but benefits most from Rust (speed)
4. **AST scanning** — lowest priority, most complex to port (needs tree-sitter)
5. **CLI** — last, because Python argparse → clap is straightforward

### Benefits at each phase

| Phase | Python needed? | Build speed | Distribution |
|-------|---------------|-------------|--------------|
| 1 (now) | Yes | Same | Same |
| 2 | Yes (wrapper) | 2-5x faster | Same |
| 3 | No | 10-50x faster | Single binary |

## xbin-core wiring (Phase 1 complete) — 2026-07-20

Stub now uses `xbin-core` as a shared library dependency instead of its local `format.rs`.

**Changes**:
- `stub/Cargo.toml` — added `xbin-core = { path = "../xbin-core" }`
- `stub/src/main.rs` — removed `mod format;`, replaced with `use xbin_core::format::{self as format, read_at, Footer};`
- `stub/src/format.rs` — **deleted** (format parsing now comes from xbin-core)
- `xbin-core/src/format.rs` — fixed 3 clippy warnings (`doc_markdown`, `format_collect`, `unnecessary_map_or`)

**Verification**:
- `stub` compiles clean with `cargo check` and `cargo clippy -- -D warnings`
- `xbin-core` — 26/26 tests pass, clippy clean
- All existing `format::FLAG_SIGNED`, `format::CRYPTO_AES_256_GCM`, `format::PAYLOAD_FORMAT_SQUASHFS` references work unchanged via `self as format` alias

**What this enables**:
- Phase 2: Python CLI can call xbin-core via PyO3 or subprocess
- Phase 3: Full Rust CLI — both stub and CLI share the same format parser
- Single source of truth for .xbin format (no more duplicate format.rs)

## Install script + upgrade command — 2026-07-20

- **File**: `scripts/install.sh` — curl-pipe-bash installer
  - Detects platform (linux-x64, linux-arm64, macos-x64, macos-arm64)
  - Fetches latest version from GitHub API
  - Downloads tar.gz from releases, verifies SHA-256 checksum
  - Installs to `/usr/local/bin/` (or `$XBIN_INSTALL_DIR`)
  - Idempotent: skips if already up-to-date
  - Usage: `curl -fsSL https://raw.githubusercontent.com/Tednoob17/x.bin/main/scripts/install.sh | bash`
- **File**: `cli/xbin/upgrade.py` — `xbin upgrade` self-update command
  - Fetches latest version from GitHub API
  - Compares against current `_XBIN_VERSION`
  - Downloads platform-specific tar.gz, verifies SHA-256
  - Replaces xbin binary in-place (with sudo if needed)
- **File**: `cli/xbin/cli.py` — `upgrade` subparser + dispatch
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

## Real-app testing — URGENT (all runtimes)

**Why**: Unit tests pass but we've never tested with real apps. Toy examples hide real bugs — missing shared libs, wrong entrypoints, broken dep resolution, env issues. We need to prove x.bin works on production apps for every runtime.

**Process per app**: `git clone` → `xbin build` → run → document pass/fail/bugs → fix → re-test

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

### Distribution & packaging
- GitHub Actions official action (`action-xbin/build`) — for CI/CD workflows

### Remaining features
- Cross-build aarch64 stub locally: requires `rustup target add aarch64-unknown-linux-musl` + cross-linker. CI handles this automatically via GitHub Actions runners.
- `xbin sign` with automatic key lookup in `$XDG_DATA_HOME/xbin/keys/` (without `--key`).
- `squashfs + mmap` direct read (kernel mount, Linux 5.12+, no extraction needed) — the real cold-start perf win beyond just better compression.
- LRU cache cleanup (evict beyond threshold)
- Cold/warm start < 100 ms end-to-end
- Distribution / discovery (lightweight registry)
- Run full end-to-end build+sign+verify cycle for aarch64 once stub is compiled locally
- GitHub Actions official action (`action-xbin/build`) — for CI/CD workflows

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
