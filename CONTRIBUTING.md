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

### Phase 1 (current MVP)
- Node.js end-to-end support
- `requirements.txt` → auto pip-install at build time
- Testing on different Linux distributions
- Performance profiling (cold/warm start times)

### Phase 2
- Ed25519 signing (`xbin keygen` / `sign` / `verify`)
- User namespaces isolation (`pivot_root`)
- AI dependency analyzer (detect `subprocess`, `dlopen` calls)
- Manifest mode (`xbin.toml`)
- LRU cache eviction

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
make stub
make example
./hello-web.xbin
```

## Development workflow

1. **Fork** the repo on GitHub.
2. **Create a branch** — `git checkout -b feature/your-feature`.
3. **Make changes.** See [code conventions](#code-conventions) below.
4. **Test your changes.** Ensure the full pipeline works end-to-end.
5. **Commit** with a clear message (see commit style below).
6. **Open a pull request** against `main`.

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

- Written in French (the builder's native language) — this is intentional
- Uses mdbook for structure
- Images go in `docs/src/images/`
- Keep the format spec in sync between `format.py` and `format.rs`

## Pull request process

1. CI must pass (lint, typecheck, end-to-end build).
2. At least one review from a maintainer.
3. New features should include a demo example or test where practical.
4. PRs that change the `.xbin` format must update **both** `format.py` and
   `format.rs`.

## Questions?

Open a [Discussion](https://github.com/tedsig42/xbin/discussions) or tag
`@tedsig42` in your issue.
