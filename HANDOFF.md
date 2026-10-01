# HANDOFF.md - daedalus project status

Condensed status doc. The historical engineering log lived here and grew to 1500 lines of
dated entries duplicating `CHANGELOG.md` plus a deleted Python CLI. That content is in git
history if it is ever needed; this file now tracks only what is true today.

Last updated: 2026-09-29

## Current state

- **Version**: 0.7.1
- **Status**: full Rust CLI, no Python in the build path
- **Format**: v5 (SquashFS supported); v2 plain, v3 signed, v4 encrypted
- **Crates**: `daedalux-core` (`daedalus-core/`), `daedalus-stub`, `daedalux`
  (`daedalus-cli/`). Note the package names lost the `d`, the directories kept it.
- **Runtimes**: 20, in detection-priority order. Mirrors `enum Runtime` in
  `daedalus-core/src/detect.rs` - if you add one, update `README.md` and
  `docs/src/roadmap.md` in the same commit.
  Python, Deno, Node, Electron, Flutter, Dart, Java, Ruby, .NET, Rust, Zig, Go, PHP, Perl,
  Hugo, Ollama, Gemma, Wasm, Binary.
- **CLI commands**: 23 + `help` (see `daedalus --help`)
- **Tests**: 640 in the workspace, 0 failures. Numbers drift, do not treat as a contract.
- **Signing**: every commit on `main` is PGP-signed (Ed25519 key
  `06CDFED6638167E3D91C19E253D20D563F8AD7EE`, id `5359795`). Local `commit.gpgsign=true`.
- **Release**: tag a release on GitHub; the workflow builds linux musl+gnu, darwin and
  windows gnu/msvc targets and uploads artifacts plus `checksums.txt`.

## Machine-specific build constraints

These are properties of this dev box, not of the project. They will bite you first.

| Constraint | Consequence | Workaround |
|---|---|---|
| Repo on vfat | No exec bit, no symlinks | Build artifacts must live outside the tree |
| `target/` redirected | Build output goes to a tmp path | See `.cargo/config.toml` |
| Tools in `~/.local/bin` | Not on default PATH | `export PATH="$HOME/.local/bin:$PATH"` |
| Stub needs musl | `cargo build` alone is not enough | `rustup target add <arch>-unknown-linux-musl`, needs a C compiler (musl-tools) |

`cargo clippy` and `cargo test` are per-crate, not workspace-wide: `daedalux-core`,
`daedalus-stub`, then `daedalux`. Using the directory name (`daedalus-core`) fails with a
package-ID error.

## Verification loop

Run before any commit. The full rules live in `AGENTS.md`; this is the short form.

```bash
cargo fmt --check
cargo clippy -p daedalux-core --all-targets -- -D warnings
cargo clippy -p daedalus-stub  --all-targets -- -D warnings
cargo clippy -p daedalux       --all-targets -- -D warnings
cargo test --workspace
```

## Known build constraints

Hard-won from packaging real apps. These are the ones that still bite.

### PHP

| Issue | Impact | Mitigation |
|---|---|---|
| `composer` absent | Build fails immediately | Auto-installed by the downloader |
| Missing extensions (ext-gd, ext-dom, ext-simplexml, ext-bcmath, ext-xml) | `composer install` exits 2 | `--ignore-platform-reqs` for portable builds |
| No `vendor/` before install | Vendor deps missing from the layer | `composer install` then refresh the site-packages plan |
| Composer version mismatch | Lock file platform requirements fail | Pin composer, or `--no-dev --ignore-platform-reqs` |
| A `package.json` alongside a PHP app | Node detection wins | Heuristic defers to PHP when `artisan` / `wp-config.php` / `symfony.lock` is present |

### Node.js

| Issue | Impact | Mitigation |
|---|---|---|
| `node` not on PATH (NVM shells) | Runtime detection fails | Fall back to `~/.nvm/versions/node/*/bin/node` |
| `pnpm` / `yarn` / `bun` absent | Lock file honored, manager missing | Fall back to `npm install` |
| npm workspaces (`workspace:*`) | `npm install` exits 1, EUNSUPPORTEDPROTOCOL | Detect workspace configs and install workspace-aware |
| Network flakiness (ECONNRESET, ETIMEDOUT) | Install dies mid-build | Retry with backoff, 3 attempts |
| `node_modules` dropped from the app layer | Dependencies not embedded | Refresh the site-packages plan after install |

### General

| Issue | Impact | Mitigation |
|---|---|---|
| Subprocess locale | Error messages in French/Chinese/etc | `--lang` flag |
| Pre-existing `vendor/` in the app layer | Copy fails on existing destination | Remove the destination before copying |
| Build on a live USB stick | No exec bit, no symlinks | Stub in `/tmp`, copy rather than symlink |
| Network timeouts during fetch | Build fails | Exponential backoff in fetch and install paths |

## Performance

Build for uptime-kuma (65 MB output) took **148s** on a Xeon w5-2465X (32 cores) before
optimization, and 5-10 minutes on a laptop. Four changes fixed it:

| Change | Before | After | Impact |
|---|---|---|---|
| zstd level | 19 | 3 | ~10x faster |
| Multithreading | none | all cores | ~Nx on N cores |
| tar to zstd | buffered in memory | direct pipe | ~50% less memory |
| `DEFAULT_LEVEL` | hardcoded 19 | 3 | single source of truth |

Post-optimization expectation: 15-25s on a 32-core Xeon, 30-60s on a typical laptop.

Peak RSS on the Xeon run was 660 MB, so an 8 GB tmpfs live USB fits, and the streaming
change should have dropped that well below the 660 MB figure since the full tar is no
longer held in memory.

Raw reports are in `benchmarks/`, plus a `benchmarks/comparison/` directory.

## Removed / deprecated

Recorded here only so nobody re-adds them.

- **Python CLI**: removed in v0.4.0. `cli/` no longer exists. The `python` feature in
  `daedalus-core/Cargo.toml` is an optional PyO3 extension only, not a build dependency.
- **OpenTelemetry**: deprecated 2026-08-21, `otel.rs` deleted.
- **Cargo package names**: `daedalux-*` on crates.io/PyPI (`daedalux` on PyPI, because
  PyPI rejected `daedalus`), `daedalus` is still the binary and module name.
