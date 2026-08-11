# Security Policy

## Supported Versions

| Version | Supported          |
|---------|--------------------|
| 0.x     | :white_check_mark: |

## Reporting a Vulnerability

**erebus is currently in Phase 3.** Security is a first-class design
concern, not an afterthought.

If you discover a vulnerability:

1. **Do not** open a public GitHub issue.
2. Email the maintainer directly or open a
   [confidential advisory](https://github.com/tedsig42/erebus/security/advisories).

You should receive a response within 48 hours. If you don't, please follow up.

## Current security posture

| Protection              | Status     | Details                                      |
|-------------------------|------------|----------------------------------------------|
| SHA-256 integrity       | ✅         | Payload hash verified before extraction      |
| Atomic cache extraction | ✅         | `rename()` avoids TOCTOU race conditions     |
| `flock()` concurrency   | ✅         | Advisory lock prevents duplicate extraction  |
| Ed25519 signatures      | ✅         | Footer v3 with keygen/sign/verify            |
| User namespaces         | ✅         | `pivot_root` isolation without root          |
| Seccomp filter          | ✅         | Denylist blocks ~14 dangerous syscalls       |
| AES-256-GCM encryption  | ✅         | Optional payload encryption (v4 format)      |

See [docs/src/security.md](docs/src/security.md) for the full threat model.

## Design principle

**Nothing touches disk before integrity is verified.** The launcher reads the
footer, validates SHA-256, and only then extracts the payload. Signatures
(Phase 2) will be verified before SHA-256 — so no data is ever processed
from an untrusted source.

## Scope

All code under `erebus-core/`, `erebus-cli/`, `stub/`, and the `.ere` format
specification in `docs/src/reference/format.md` is in scope. The example
apps in `examples/` are for demonstration only and not considered
security-critical. The legacy Python CLI in `cli/` is deprecated.
