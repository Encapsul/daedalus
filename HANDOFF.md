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

## Next steps (future)

- `xbin sign` with automatic key lookup in `~/.xbin/keys/` (without `--key`).
- `xbin verify` using launcher embedded logic (via `$XBIN_TRUSTED_DIR`).
- Support for `--key-dir` default in `xbin keygen`.
- Possibly a `trust` subcommand to manage trusted keys.
