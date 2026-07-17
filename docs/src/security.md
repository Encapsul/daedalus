# Security

A tool that distributes and executes code needs security in the
architecture, not bolted on afterward. This page documents each attack
surface, the naive flaw, and the current defense.

Status: SHA-256 integrity and atomic extraction are in the MVP. Ed25519
signatures are implemented (Phase 2). Seccomp filtering is planned (Phase 3).

## 1. Authenticity — Ed25519 signatures

**Attack:** Anyone can produce a `.xbin`. The user has no way to verify
where it came from or whether it was modified.

**Defense:** Every `.xbin` can be signed. The launcher verifies the
signature **before extracting anything**. Invalid or missing signature →
refusal to execute.

```bash
# Generate a keypair
$ xbin keygen --key-dir ~/.xbin/keys
a1b2c3d4e5f6...

# Sign a .xbin (in-place, writes v3 footer)
$ xbin sign my_app.xbin --key ~/.xbin/keys/a1b2c3d4e5f6.key
[xbin] signed my_app.xbin

# Verify before running
$ xbin verify my_app.xbin --trusted-dir ~/.xbin/trusted-keys
[xbin] signature verified for /path/to/my_app.xbin
```

**Why Ed25519 over RSA:**
- Short keys (32 bytes) and signatures (64 bytes) — minimal overhead in the
  footer.
- Timing-attack resistant by design (constant-time scalar multiplication).
- Standard in modern protocols (SSH, TLS 1.3, Signal, WireGuard).

**Trust model:** Trusted public keys live in `~/.xbin/trusted-keys/`. The
launcher accepts the file if **any** trusted key verifies the signature.
There is no central authority — trust is local and explicit.

```bash
# Trust a key
$ xbin trust ~/.xbin/keys/a1b2c3d4e5f6.pub
[xbin] trusted key a1b2c3d4e5f6...
```

## 2. Cache race condition — TOCTOU

**Attack:** Between checking that a cache entry exists and using it, an
attacker could substitute the extracted rootfs with a malicious one.

**Defense:** Extraction writes to a unique temp directory, then atomically
renames it to the final cache path. No intermediate state is ever visible
to other processes.

```
1. extract to  ~/.cache/xbin/.tmp-{pid}-{nanos}/   ← unique, private
2. write .ready marker
3. rename() to ~/.cache/xbin/{sha256}/              ← atomic on Linux
4. if another instance won the race → discard our tmp
```

`rename()` on the same filesystem is atomic: either the target exists and
is complete, or it doesn't. See [Cache](./reference/cache.md) for details.

## 3. Integrity — SHA-256

**Attack:** The payload is corrupted in transit (bit flip, truncation,
incomplete download).

**Defense:** The launcher recomputes `SHA-256(payload ‖ metadata)` and
compares it to the footer's `payload_sha256` **before** extraction. On
mismatch: `exit(1)`, nothing written to disk.

```bash
# Corrupt one byte of a signed .xbin
$ dd if=/dev/urandom of=my_app.xbin bs=1 seek=688788 count=1
$ xbin verify my_app.xbin
[xbin] error: signature verification FAILED for /path/to/my_app.xbin
```

**Limitation:** SHA-256 alone protects against **corruption**, not against
an attacker who modifies the payload and recomputes the hash. This is why
signatures exist — the hash is signed:

```
Ed25519_sign(SHA-256(payload ‖ metadata), private_key)
```

Without the private key, forging a valid signature is computationally
infeasible (2^128 operations for Ed25519).

## 4. LD_LIBRARY_PATH fallback (level 0)

**Attack:** At isolation level 0, the app sees the host filesystem via
`LD_LIBRARY_PATH`. A malicious `.so` placed in a searched directory
(`/lib`, `/usr/lib`) could be loaded instead of the real one.

**Current status:** Level 0 is the default because it works everywhere
without privileges. Level 2 (user namespaces + `pivot_root`) eliminates
this entirely — the app sees only its own rootfs.

**Open question:** If user namespaces are unavailable on the target,
should we refuse to execute (security-first) or fall back to level 0 with
a warning (UX-first)? The answer depends on the deployment context.

See [Isolation](./reference/isolation.md) for the full comparison.

## Secure execution sequence

```
1. open("/proc/self/exe")               ← self-locate (not argv[0])
2. read footer, validate magic           ← reject early if not .xbin
3. ── VERIFY Ed25519 SIGNATURE ──        ← nothing executes before this
4. verify payload SHA-256                ← corruption check
5. atomic extraction (tmp → rename)      ← no TOCTOU window
6. user namespace + pivot_root           ← filesystem isolation (level 2)
7. seccomp filter                        ← syscall filtering (Phase 3)
8. exec entrypoint
```

**Rule:** Nothing is written to disk before integrity is verified. With
signatures enabled, nothing executes before the signature is verified.
