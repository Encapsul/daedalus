# Contributing to daedalus

Thanks for your interest! daedalus is a young project and contributions are
welcome — whether it's code, docs, bug reports, or design discussion.

## Quick links

- [Code of Conduct](#code-of-conduct)
- [What we need help with](#what-we-need-help-with)
- [Getting started](#getting-started)
- [Development workflow](#development-workflow)
- [Code conventions](#code-conventions)
- [Pull request process](#pull-request-process)

## Code of Conduct

Be respectful, constructive, and assume good faith. Harassment or
discriminatory behavior will not be tolerated.

## What we need help with

### Phase 3 (current)
- SquashFS + mmap direct read (kernel mount, no extraction)
- LRU cache eviction
- Cold/warm start < 100 ms
- Distribution / discovery (lightweight registry)

### Always welcome
- Documentation improvements
- Bug reports with reproduction steps
- Example apps in different languages/runtimes
- Architecture diagram improvements

## Getting started

```bash
# Prerequisites
rustup target add x86_64-unknown-linux-musl

# Build and test
make preflight
make stub
make example
./hello-web.daedalus
```

## Branching strategy

```
main          ← stable, tagged releases only
  └── dev     ← integration branch, all PRs target here
       ├── feat/squashfs-mmap
       ├── feat/registry
       ├── fix/pip-cross-download
       └── ...
```

- **`main`** — production-ready. Only `dev` merges into `main` via PR.
  Every push to `main` triggers a release if tagged `v*`.
- **`dev`** — active development. All feature branches merge here.
  CI runs on every push. This is the default branch for PRs.
- **`feat/*`** — feature branches. One feature per branch.
  Branch off `dev`, PR back into `dev`.
- **`fix/*`** — bug fix branches. Same flow as `feat/*`.

### Workflow

1. Create a branch from `dev`:
   ```bash
   git checkout dev && git pull
   git checkout -b feat/my-feature
   ```
2. Make changes, commit, push.
3. Open a PR against `dev`.
4. CI must pass (lint, build, end-to-end test).
5. Merge via squash or rebase (no merge commits).
6. Periodically, `dev` merges into `main` for releases.

### Commit style

We use [Conventional Commits](https://www.conventionalcommits.org/):

```
type(component): brief description (imperative mood)

Optional longer description. Why the change was made, what it
does differently, any trade-offs.
```

Types: `feat`, `fix`, `chore`, `docs`, `refactor`, `test`, `ci`, `perf`
Good: `feat(build): add auto pip-install from requirements.txt`
Bad: `fix stuff` or `updated code`

## Code conventions

### Python (`cli/`)

- Target **Python ≥ 3.10** (stdlib only for MVP — no Click, Rich, etc.)
- Type hints required on all public functions
- `from __future__ import annotations` at the top
- Docstrings in English, triple-quoted, with brief description
- Keep functions focused and small
- Progress/status output goes to **stderr** (`file=sys.stderr`), never stdout
- Use `_color.py` helpers (red, green, yellow, bold) for terminal output — respects `--no-color` / `NO_COLOR`
- Non-TTY stderr: suppress verbose progress (use `verbose` parameter)
- Interactive prompts: always offer `--force` / `-f` to skip (for CI/scripting)
- Machine-readable output: offer `--json` flag where applicable (inspect, doctor)
- XDG compliance: use `_util.keys_dir()` / `_util.trusted_dir()` for config paths

### Rust (`daedalus-core/`, `daedalus-cli/`, `stub/`)

- **No `unsafe` in `daedalus-core`** — all unsafe is confined to `stub/src/main.rs`
- `stub/` unsafe requires a `SAFETY` comment explaining soundness
- Pure Rust dependencies only (no C toolchain required)
- Comments in English, doc comments (`///`) on public items
- Follow standard Rust formatting (`cargo fmt`)

### Documentation (`docs/`)

- Written in English
- Uses mdbook for structure
- Images go in `docs/src/images/`

## Pull request process

1. PR targets `dev` (not `main`).
2. CI must pass (lint, clippy, build, end-to-end test).
3. At least one review from a maintainer.
4. New features should include a demo example or test where practical.
5. PRs that change the `.daedalus` format must update `daedalus-core/src/format.rs`.
6. Squash or rebase — no merge commits on `dev`.

### Release process

```bash
./scripts/release.sh 0.3.2    # creates v0.3.2 tag, pushes, CI builds binaries
```

The release CI builds `daedalus-stub` for:
- Linux x86_64 (`x86_64-unknown-linux-musl`)
- Linux aarch64 (`aarch64-unknown-linux-gnu`)

Each archive contains `bin/daedalus-stub` (statically linked, no dependencies). A GitHub Release is created with binaries + SHA-256 checksums.

## Questions?

Open a [Discussion](https://github.com/Tednoob17/daedalus/discussions) or tag
`@Tednoob17` in your issue.
