# Security Policy

## Supported Versions

| Version | Supported          |
|---------|--------------------|
| 0.x     | :white_check_mark: |

## Reporting a Vulnerability

**daedalus is currently in Phase 3.** Security is a first-class design
concern, not an afterthought.

If you discover a vulnerability:

1. **Do not** open a public GitHub issue.
2. Email the maintainer directly or open a
   [confidential advisory](https://github.com/tedsig42/daedalus/security/advisories).

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
| Seccomp filter          | ✅         | Denylist blocks ~14 dangerous syscalls       |
| AES-256-GCM encryption  | ✅         | Optional payload encryption (v4 format)      |

See [docs/src/security.md](docs/src/security.md) for the full threat model.

## Security audit

| Date       | Scope | Result |
|------------|-------|--------|
| 2026-09-09 | Ed25519 signature verification paths: `daedalus verify`, stub launcher, SISR delta verification, `daedalus-crypto` util | **One issue found & fixed.** All four paths verified with `ed25519_dalek::Verifier::verify` (non-strict, cofactorless), which accepts small-order public keys and signatures (ZIP-215/weak-key malleability). Switched every call site to `verify_strict`, which rejects small-order `R` and small-order trusted keys. Valid signatures are unaffected; all existing verify/sign/SISR/chaos tests pass. Severity was LOW in practice — the trust anchor is operator-controlled — but strict verification removes the malleability class for free. |

Audit checks that passed with no action needed:

- **Digest covers the footer** — `SHA-256(payload ‖ metadata ‖ footer.pack_full())`; a format-downgrade (v3→v2) or flag-clearing attack invalidates the signature.
- **Trust anchor path parity** — CLI `verify`, `trust`, and the stub launcher all resolve keys via `daedalus_core::paths::trusted_keys_dir()` (`$DAEDALUS_TRUSTED_DIR` override; default `~/.daedalus/trusted-keys/`, HOME-relative so sandboxed/elevated contexts can't inject a spoofed env var).
- **No secrets in the binary or repo** — private dev keys live outside the artifact in `~/.local/share/daedalus/keys/` (mode 0600), public key only is embedded/trusted. No hardcoded keys found in `daedalus-core`, `daedalus-cli`, or `daedalus-stub`.
- **Config-fingerprint isolation** — signed vs `--skip-sign` builds produce distinct cache keys, so a cache hit can never serve an unsigned artifact for a signed request.
- **Constant-time SHA-256 comparison** — `verify_sha256` uses XOR-fold accumulation instead of early-exit memcmp (no timing side-channel).

## Design principle

**Nothing touches disk before integrity is verified.** The launcher reads the
footer, validates SHA-256, and only then extracts the payload. Signatures are
verified before SHA-256 — so no data is ever processed from an untrusted
source. Signing is **on by default**: every `daedalus build` is signed with a
self-generated, self-trusted dev key, so `daedalus verify foo.de` authenticates
an artifact out of the box. Pass `--skip-sign` (or `--key <path>` for a
specific key) to opt out or override.

## Scope

All code under `daedalus-core/`, `daedalus-cli/`, `stub/`, and the `.daedalus` format
specification in `docs/src/reference/format.md` is in scope. The example
apps in `examples/` are for demonstration only and not considered
security-critical. The legacy Python CLI in `cli/` is deprecated.
