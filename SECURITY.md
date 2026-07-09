# Security Policy

## Supported Versions

| Version | Supported          |
|---------|--------------------|
| 0.x     | :white_check_mark: |

## Reporting a Vulnerability

**xbin is currently in MVP/Phase 1.** Security is a first-class design
concern, not an afterthought, but some protections are still being built.

If you discover a vulnerability:

1. **Do not** open a public GitHub issue.
2. Email the maintainer directly or open a
   [confidential advisory](https://github.com/tedsig42/xbin/security/advisories).

You should receive a response within 48 hours. If you don't, please follow up.

## Current security posture

| Protection              | Status     | Details                                      |
|-------------------------|------------|----------------------------------------------|
| SHA-256 integrity       | ✅ MVP     | Payload hash verified before extraction      |
| Atomic cache extraction | ✅ MVP     | `rename()` avoids TOCTOU race conditions     |
| `flock()` concurrency   | ✅ MVP     | Advisory lock prevents duplicate extraction  |
| Ed25519 signatures      | 🔜 Phase 2 | Footer v2 with keygen/sign/verify            |
| User namespaces         | 🔜 Phase 2 | `pivot_root` isolation without root          |
| Seccomp filter          | 🔜 Phase 2 | Restrict syscalls available to embedded app  |

See [docs/src/securite.md](docs/src/securite.md) for the full threat model.

## Design principle

**Nothing touches disk before integrity is verified.** The launcher reads the
footer, validates SHA-256, and only then extracts the payload. Signatures
(Phase 2) will be verified before SHA-256 — so no data is ever processed
from an untrusted source.

## Scope

All code under `cli/`, `stub/`, and the `.xbin` format specification in
`docs/src/reference/format.md` is in scope. The example apps in `examples/`
are for demonstration only and not considered security-critical.
