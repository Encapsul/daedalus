# Security Policy

## Supported Versions

| Version | Supported          |
|---------|--------------------|
| 0.x     | :white_check_mark: |

## Reporting a Vulnerability

daedalus is at 0.7.1. Report vulnerabilities in private.

If you discover a vulnerability:

1. **Do not** open a public GitHub issue.
2. Email the maintainer directly or open a
   [confidential advisory](https://github.com/Encapsul/daedalus/security/advisories).

You should receive a response within 48 hours. If you don't, please follow up.

## Current security posture

| Protection              | Status     | Details                                      |
|-------------------------|------------|----------------------------------------------|
| SHA-256 integrity       | ✅         | Payload hash verified before extraction      |
| Atomic cache extraction | ✅         | `rename()` avoids TOCTOU race conditions     |
| `flock()` concurrency   | ✅         | Advisory lock prevents duplicate extraction  |
| Ed25519 signatures      | ✅         | Footer v3; **signing ON by default**         |
| Default signing         | ✅         | Auto-generated dev key + self-trust; `--skip-sign` opts out |
| Strict Ed25519 verify   | ✅         | `verify_strict` (rejects small-order keys/sigs) |
| User namespaces         | ✅         | `pivot_root` isolation without root          |
| Seccomp filter          | ✅         | Denylist of 18 syscalls. Not an app sandbox. Warns and continues if it cannot install |
| AES-256-GCM encryption  | ✅         | Optional. Decryption key is in file metadata so the stub can decrypt |

See [docs/src/security.md](docs/src/security.md) for the full threat model.

## Security audit

| Date       | Scope | Result |
|------------|-------|--------|
| 2026-09-09 | Ed25519 signature verification paths: `daedalus verify`, stub launcher, SISR delta verification, `daedalus-crypto` util | **One issue found & fixed.** All four paths verified with `ed25519_dalek::Verifier::verify` (non-strict, cofactorless), which accepts small-order public keys and signatures (ZIP-215/weak-key malleability). Switched every call site to `verify_strict`, which rejects small-order `R` and small-order trusted keys. Valid signatures are unaffected; all existing verify/sign/SISR/chaos tests pass. Severity was LOW in practice - the trust anchor is operator-controlled - but strict verification removes the malleability class for free. |

Audit checks that passed with no action needed:

- **Digest covers the footer** - `SHA-256(payload ‖ metadata ‖ footer.pack_full())`; a format-downgrade (v3→v2) or flag-clearing attack invalidates the signature.
- **Trust anchor path parity** - CLI `verify`, `trust`, and the stub launcher all resolve keys via `daedalus_core::paths::trusted_keys_dir()` (`$DAEDALUS_TRUSTED_DIR` override; default `~/.daedalus/trusted-keys/`, HOME-relative so sandboxed/elevated contexts can't inject a spoofed env var).
- **No secrets in the binary or repo** - private dev keys live outside the artifact in `~/.local/share/daedalus/keys/` (mode 0600), public key only is embedded/trusted. No hardcoded keys found in `daedalus-core`, `daedalus-cli`, or `daedalus-stub`.
- **Config-fingerprint isolation** - signed vs `--skip-sign` builds produce distinct cache keys, so a cache hit can never serve an unsigned artifact for a signed request.
- **Constant-time SHA-256 comparison** - `verify_sha256` uses XOR-fold accumulation instead of early-exit memcmp (no timing side-channel).

## Design principle

**Nothing touches disk before integrity is verified.** The launcher reads the
footer, validates SHA-256, and only then extracts the payload. When a
signature block is present, signature verification runs before that hash
check.

Signing is **on by default** with a dev key created on the build machine and
added to that machine's trust store. `daedalus verify` there succeeds because
of that auto-trust. A recipient authenticates the file only after
`daedalus trust` of the matching `.pub`. Pass `--skip-sign`, or `--key` for a
specific key.

## Scope

All code under `daedalus-core/`, `daedalus-cli/`, `daedalus-stub/`, and the
format specification in `docs/src/reference/format.md` is in scope. The example
apps in `examples/` are for demonstration only and not considered
security-critical.
