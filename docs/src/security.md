# Security

A tool that distributes and executes code is a natural target. `xbin`'s
security must be **in the architecture**, not bolted on. Below are the flaws
of the naive design and their defenses. (Signatures are Phase 2; SHA-256
integrity and atomic extraction are **already** in the MVP.)

## 1. Authenticity — Ed25519 signature ✅ (Phase 2)

**Problem**: anyone can forge a `.xbin`. The user has no way to know where
it came from.

**Defense**: every `.xbin` is signed. The launcher verifies the signature
**before extracting anything**. Invalid or missing signature → refusal.

Why Ed25519 over RSA: short keys/sigs (32/64 bytes), fast verification,
timing-attack resistant. It's the modern standard (SSH, TLS 1.3, Signal).

## 2. Cache race condition — TOCTOU ✅ (already handled)

**Problem**: between checking cache existence and using it, an attacker could
substitute content.

**Defense**: extraction to a unique temp directory, then **atomic** `rename()`
to the final cache. No intermediate state exposed. See
[Cache](./reference/cache.md).

## 3. Integrity — SHA-256 ✅ (already handled)

The launcher recomputes the payload's SHA-256 and compares it to the footer's
**before** extraction. On mismatch: `exit(1)`, nothing is written to disk.

> SHA-256 alone protects against **corruption**, not against an attacker who
> would recompute the hash after modification. This is precisely why Phase 2
> **signs** the hash: `Ed25519_sign(SHA256(payload+meta), private_key)`.
> Without the private key, forging a valid signature is impossible.

## 4. Unsafe `LD_LIBRARY_PATH` fallback

**Problem**: level 0 mode lets the app see the host filesystem; a fake lib
placed in a visible directory could be loaded.

**Target defense**: at level 2 (user namespaces + `pivot_root`), the app sees
**only** its rootfs. Open question: if user namespaces are unavailable,
should we refuse (security-first) or fall back to level 0 with a warning
(UX-first)? The choice depends on the market — see [Isolation](./reference/isolation.md).

## The target secure sequence

```
1. open(/proc/self/exe)
2. read footer, validate magic
3. ── VERIFY Ed25519 SIGNATURE ──     ← nothing happens before (Phase 2)
4. verify payload SHA-256              ← already in MVP
5. atomic extraction (tmp → rename)    ← already in MVP
6. user namespace + pivot_root         ← Phase 2
7. seccomp filter                       ← Phase 3
8. exec entrypoint
```

> **Fundamental rule**: nothing is written to disk before integrity (MVP) —
> and soon the signature (Phase 2) — is verified.
