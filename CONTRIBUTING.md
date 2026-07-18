# Contributing to xbin

Thanks for your interest! xbin is a young project and contributions are
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
python3 -m pip install -e cli/

# Build and test
make preflight
make stub
make example
./hello-web.xbin
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

```
component: brief description (imperative mood)

Optional longer description. Why the change was made, what it
does differently, any trade-offs.
```

Good: `builder: add auto pip-install from requirements.txt`
Bad: `fix stuff` or `updated code`

## Code conventions

### Python (`cli/`)

- Target **Python ≥ 3.10** (stdlib only for MVP — no Click, Rich, etc.)
- Type hints required on all public functions
- `from __future__ import annotations` at the top
- Docstrings in English, triple-quoted, with brief description
- Keep functions focused and small

### Rust (`stub/`)

- **No `unsafe`** unless absolutely necessary (and documented why)
- Pure Rust dependencies only (no C toolchain required)
- Comments in English, doc comments (`///`) on public items
- Follow standard Rust formatting (we use `rustfmt`)

### Documentation (`docs/`)

- Written in English
- Uses mdbook for structure
- Images go in `docs/src/images/`
- Keep the format spec in sync between `format.py` and `format.rs`

## Pull request process

1. PR targets `dev` (not `main`).
2. CI must pass (lint, clippy, build, end-to-end test).
3. At least one review from a maintainer.
4. New features should include a demo example or test where practical.
5. PRs that change the `.xbin` format must update **both** `format.py` and
   `format.rs`.
6. Squash or rebase — no merge commits on `dev`.

### Release process

```bash
./scripts/release.sh 0.1.0    # creates v0.1.0 tag, pushes, CI builds binaries
```

The release CI builds `xbin-stub` + `xbin-crypto` for:
- Linux x86_64 (`x86_64-unknown-linux-musl`)
- Linux aarch64 (`aarch64-unknown-linux-musl`)
- macOS ARM64 (`aarch64-apple-darwin`)
- macOS x64 (`x86_64-apple-darwin`)

Each archive contains `bin/xbin-stub` and `bin/xbin-crypto` (statically linked, no dependencies). A GitHub Release is created with binaries + SHA-256 checksums.

## Questions?

Open a [Discussion](https://github.com/Tednoob17/x.bin/discussions) or tag
`@Tednoob17` in your issue.
