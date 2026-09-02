# Testing

daedalus ships four layers of tests. They run on a stable toolchain only, and
the full suite fits comfortably under 30 seconds in CI, with no root access.

## Quick reference

```bash
# Formatting
cargo fmt --check

# Lint (per-crate, as CI does)
cargo clippy -p daedalus-core --all-targets -- -D warnings
cargo clippy -p daedalus-stub --all-targets -- -D warnings
cargo clippy -p daedalus-cli --all-targets -- -D warnings

# Everything
cargo test --workspace

# Docs build
mdbook build docs/
```

## Layer 1 — unit tests

`#[cfg(test)] mod tests` inside each module of `daedalus-core`. Covers the chunker,
Merkle roots, tar/zstd round-trips, footer codec, and parser edge cases.

## Layer 2 — property-based tests (proptest)

`#[cfg(test)] mod proptests` in `daedalus-core/src/manifest.rs`,
`daedalus-core/src/sisr_header.rs`, and `daedalus-core/src/format.rs`. Each test runs
256 generated cases:

- arbitrary bytes never panic any parser (`DeltaManifest::parse`,
  `SisrFooterExt::parse`, `Footer::read_from`, `read_sisr`);
- `pack`/`parse` and `serialize`/`parse` round-trips are lossless;
- truncated buffers are always rejected, never a panic.

Because proptest runs on stable, it is the primary fuzz surface in CI.
`libFuzzer` targets live in [`fuzz/`](#libfuzzer-nightly-only) for nightly
toolchains.

```bash
cargo test -p daedalus-core --lib
```

## Layer 3 — SISR engine fault injection

`daedalus-core/src/sisr/network_test.rs` drives the real engine through a
`ChunkFetcher` wrapper that injects network faults:

- latency (`thread::sleep`) — result unchanged;
- abrupt connection drops (`io::ErrorKind::ConnectionReset`) — update fails,
  binary untouched;
- corrupted packets — fails SHA-256 verification;
- truncated packets — fails the length check;
- slow throughput — still reconstructs correctly;
- fetched-byte accounting.

```bash
cargo test -p daedalus-core --lib sisr::network_test
```

## Layer 4 — end-to-end (stub integration)

`stub/tests/e2e_sisr/` launches the real `daedalus-stub` binary
(`CARGO_BIN_EXE_daedalus-stub`) against a standard-library TCP mock HTTP server.
No root, no network access: `XDG_CACHE_HOME`/`XDG_DATA_HOME` isolate the
cache, `DAEDALUS_TRUSTED_DIR` pins trusted keys, and
`DAEDALUS_HEALTH_TIMEOUT_MS` keeps the health gate fast.

```bash
cargo test -p daedalus-stub --test e2e_sisr_main
```

`update_basic.rs` covers the happy path (v1→v2 swap, delta-bound downloads
with ≤ 2% overhead on modified bytes, the new payload actually runs, and the
mission-6 local staging path still applies).

`update_failures.rs` covers every refusal path — untrusted signature, corrupt
manifest, missing chunk (404), truncated chunk, corrupted chunk bytes, and
Merkle-root mismatch — and asserts the previous binary stays intact with no
residual `.bak`.

The health-gate rollback E2E lives separately:

```bash
cargo test -p daedalus-stub --test health_rollback
```

## CI notes

- Run the full suite with `cargo test --workspace`; it must finish in well
  under 30 seconds.
- No test requires root, network access, or a running daemon.
- The fuzz harness (`daedalus-fuzz/`) is not a workspace member and is excluded
  from the stable CI lint/test loop: it needs extra dependencies (tokio, clap,
  reqwest) and its inputs are already covered by the proptest layer on stable.

## Fuzzing

`daedalus-fuzz/` is a standalone crate (its own workspace) with a deterministic
harness (`daedalus-harness`) that exercises the in-memory parsers of
`daedalus-core` (format footer, stub metadata, crypto, SISR). It never panics:
all invalid inputs are rejected cleanly. It is fully portable and runs on
Linux, macOS, and Windows.

Build and run a short smoke fuzz with a fixed seed and duration so results are
comparable across machines:

```bash
cargo run --release --manifest-path daedalus-fuzz/Cargo.toml --bin daedalus-harness -- \
  --targets format,stub,cli,crypto,sisr \
  --duration 2m \
  --seed 2413712331968747354
```

List available options (targets, workers, corpus dir, minimization):

```bash
cargo run --manifest-path daedalus-fuzz/Cargo.toml --bin daedalus-harness -- --help
```

A `fuzz-smoke` GitHub Actions workflow runs the same command on native
Linux, macOS, and Windows runners (weekly schedule + manual dispatch) so the OSes
can be diffed against the same seed. `cargo-fuzz` (nightly-only, see below) is
an alternative that the stable-only policy keeps out of CI.
