# Contributing to daedalus

Thanks for helping. Bug reports, docs fixes, examples, and focused PRs are
welcome. Read [SECURITY.md](SECURITY.md) before filing anything that looks
like a vulnerability.

## Quick links

- [Code of Conduct](CODE_OF_CONDUCT.md)
- [What we need help with](#what-we-need-help-with)
- [Getting started](#getting-started)
- [Pull requests](#pull-requests)
- [Code conventions](#code-conventions)

## Code of Conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md). Be
respectful and assume good faith.

## What we need help with

### Engineering focus

- SquashFS mount with no extraction (this is what would move cold start)
- LRU cache eviction for `~/.cache/daedalus`
- A non-hello app in the CI smoke (today: `examples/hello-web` only)
- Splitting `daedalus-stub/src/main.rs` (~2 400 lines; security boundary)

### Always welcome

- Documentation that matches what the code does
- Bug reports with a reproduction
- Small example apps under `examples/`
- Clarifying error messages

Do not open PRs that only add a new runtime detector. Detection without a CI
smoke is how the "20 runtimes" claim got ahead of the product.

## Getting started

```bash
# Linux CI target for the stub
rustup target add x86_64-unknown-linux-musl

make preflight
make stub
make example
./hello-web.de
```

Verification before you open a PR:

```bash
cargo fmt --check
cargo clippy -p daedalux-core --all-targets -- -D warnings
cargo clippy -p daedalus-stub --all-targets -- -D warnings
cargo clippy -p daedalux --all-targets -- -D warnings
cargo test --workspace
```

See [AGENTS.md](AGENTS.md) for build gotchas (vfat target dir, musl, pinned
toolchain) and [CODE_STYLE.md](CODE_STYLE.md) for Rust style.

## Pull requests

Default branch is `main`. CI runs on pushes and PRs to `main`.

1. Fork and branch from `main`: `git checkout -b feat/short-name`
2. Keep the change focused. One concern per PR.
3. Use [Conventional Commits](https://www.conventionalcommits.org/):
   `feat(build): …`, `fix(stub): …`, `docs: …`
4. Open the PR against `main`. Fill in the template.
5. CI must pass (fmt, clippy, tests, smoke).
6. Prefer squash merge.

PRs that change the on-disk `.de` / `.daedalus` footer must update
`daedalus-core/src/format.rs` and the format docs in the same change.

### Release

```bash
./scripts/release.sh 0.7.2
```

That tags `v0.7.2` and lets `.github/workflows/release.yml` publish the
binaries and `checksums.txt`.

## Code conventions

### Rust (`daedalus-core/`, `daedalus-cli/`, `daedalus-stub/`)

- No `unsafe` in `daedalus-core` or `daedalus-cli`
- `unsafe` in `daedalus-stub` needs a `SAFETY` comment
- `cargo fmt` is authoritative
- Prefer `Result` over panics in library code

### Docs (`docs/`)

- English in `docs/src/`
- Built with mdBook

## Questions

Open a [Discussion](https://github.com/Encapsul/daedalus/discussions) or an
issue with the question template.
