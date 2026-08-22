# Security

A tool that distributes and executes code needs security built into its architecture. This page documents each attack surface, the naive flaw, and the current defense.

Status: SHA-256 integrity, atomic extraction, Ed25519 signatures, user namespaces + pivot_root, seccomp-bpf denylist, and AES-256-GCM encryption are all implemented.

## 1. Authenticity — Ed25519 signatures

**Attack:** Anyone can produce a `.ere`, and the user cannot verify its origin or whether it was modified.

**Defense:** Every `.ere` can be signed. The launcher verifies the signature before extracting anything. Invalid or missing signatures result in execution refusal.

```bash
# Generate a keypair
$ daedalus keygen --key-dir $XDG_DATA_HOME/daedalus/keys
a1b2c3d4e5f6...

# Sign a .ere (in-place, writes v3 footer)
$ daedalus sign my_app.ere --key $XDG_DATA_HOME/daedalus/keys/a1b2c3d4e5f6.key
[daedalus] signed my_app.ere

# Verify before running
$ daedalus verify my_app.ere --trusted-dir $XDG_DATA_HOME/daedalus/trusted-keys
[daedalus] signature verified for /path/to/my_app.ere
```

**Why Ed25519 over RSA:**
- Short keys (32 bytes) and signatures (64 bytes) — minimal overhead in the footer.
- Timing-attack resistant by design (constant-time scalar multiplication).
- Standard in modern protocols (SSH, TLS 1.3, Signal, WireGuard).

**Trust model:** Trusted public keys live in `$XDG_DATA_HOME/daedalus/trusted-keys/`. The launcher accepts the file if any trusted key verifies the signature. There is no central authority — trust is local and explicit.

```bash
# Trust a key
$ daedalus trust $XDG_DATA_HOME/daedalus/keys/a1b2c3d4e5f6.pub
[daedalus] trusted key a1b2c3d4e5f6...
```

## 1b. Chain of Trust for Local Rebuilding

SISR (Self-Incremental Sovereign Reconstruction) lets a `.ere` rebuild
itself from signed deltas, with zero host dependency. This section fixes the
cryptographic trust model for that path. The full conceptual specification is
in [SISR: Self-Incremental Sovereign Reconstruction](./architecture/sisr-spec.md).

**Attack:** A `.ere` that can reconstruct itself becomes a remote-code
execution surface. A malicious or replayed delta could replace the binary with
an attacker-controlled (or simply old, vulnerable) version. On top of the
original integrity concern (modification in transit), self-reconstruction adds
three threats:

1. **Manifest spoofing** — a forged or edited manifest passes off arbitrary
   content as an official update.
2. **Replay** — a signed but *old* delta rolls the binary back to a
   vulnerable version.
3. **Cache poisoning** — a malicious block injected into the local
   reconstruction cache replaces legitimate content.

**Defense — three linked mechanisms:**

1. **Signed chain of trust (non-repudiation).** A `.ere` only applies a
   delta or manifest signed by the **same Ed25519 public key** that signed
   the original binary, or by a **delegated key** whose delegation record is
   itself signed by the anchor (or transitively by a valid delegate). The
   anchor is carried inside the signed region of the original file, so it
   cannot be swapped without breaking the original signature.

2. **Monotonic anti-rollback index.** Every signed manifest carries a
   monotonic version counter, stored as part of the *signed* body. The engine
   persists the highest applied index and rejects any manifest whose index is
   lower or equal. Replaying an old but validly-signed update is therefore
   impossible — and the counter cannot be lowered by editing the file without
   invalidating its signature.

3. **Verify-before-assemble (CDC/Merkle).** Each downloaded block is verified
   against its content hash (Content-Defined Chunking) before assembly; blocks
   are addressed by content, so a malicious block produces a different key and
   is rejected. The engine assembles a candidate version, verifies the full
   result matches the signed `target_sha256`, and only then commits it
   atomically.

**Non-negotiable verification order (every reconstruction):**

```
1. self-locate ( /proc/self/exe )
2. read manifest header, validate magic
3. ── VERIFY SIGNATURE over the signed body ──   ← nothing runs before this
4. verify monotonic index > highest applied index  ← anti-rollback
5. fetch blocks + verify each block's content hash  ← CDC/Merkle
6. ── ASSEMBLE CANDIDATE (pure, no live mutation) ──
7. verify candidate hash == target_sha256
8. ── ATOMIC COMMIT ──                             ← interrupt-safe swap
```

Any failure anywhere aborts the entire chain: the previous binary remains
byte-identical and functional. **Unsigned deltas are never accepted** —
`allow_unsigned = false` is a hard invariant. See
[the SISR spec](./architecture/sisr-spec.md#7-the-sisrrebuilder-contract) for
the full contract.

## 2. Cache race condition — TOCTOU

**Attack:** Between checking that a cache entry exists and using it, an attacker could substitute the extracted rootfs with a malicious one.

**Defense:** Extraction writes to a unique temp directory, then atomically renames it to the final cache path. No intermediate state is ever visible to other processes.

```
1. extract to ~/.cache/daedalus/.tmp-{pid}-{nanos}/   ← unique, private
2. write .ready marker
3. rename() to ~/.cache/daedalus/{sha256}/              ← atomic on Linux
4. if another instance won the race → discard our tmp
```

`rename()` on the same filesystem is atomic: either the target exists and is complete, or it doesn't. See [Cache](./reference/cache.md) for details.

## 3. Integrity — SHA-256

**Attack:** The payload is corrupted in transit (bit flip, truncation, incomplete download).

**Defense:** The launcher recomputes `SHA-256(payload ‖ metadata)` and compares it to the footer's `payload_sha256` before extraction. On mismatch: `exit(1)`, nothing written to disk.

```bash
# Corrupt one byte of a signed .ere
$ dd if=/dev/urandom of=my_app.ere bs=1 seek=688788 count=1
$ daedalus verify my_app.ere
[daedalus] error: signature verification FAILED for /path/to/my_app.ere
```

SHA-256 alone protects against corruption, not against an attacker who modifies the payload and recomputes the hash. This is why signatures exist — the hash is signed:

```
Ed25519_sign(SHA-256(payload ‖ metadata ‖ footer), private_key)
```

The footer is part of the digest because it decides whether the signature is
consulted at all (`format_version >= 3` + `FLAG_SIGNED`). A signature over
`payload ‖ metadata` alone would let an attacker downgrade the file to v2,
clear the flag, and recompute the SHA-256 — the signature would be silently
skipped. Covering the footer makes any such tampering invalidate the
signature; the launcher additionally rejects inconsistent states (sig block
present without `FLAG_SIGNED`, or flag set without a block) and v2 files that
still carry a leftover signature block from a downgrade.

Without the private key, forging a valid signature is computationally infeasible (2^128 operations for Ed25519).

## 4. LD_LIBRARY_PATH fallback (level 0)

**Attack:** At isolation level 0, the app sees the host filesystem via `LD_LIBRARY_PATH`. A malicious `.so` placed in a searched directory (`/lib`, `/usr/lib`) could be loaded instead of the real one.

**Current status:** Level 0 is the default because it works everywhere without privileges. Level 2 (user namespaces + `pivot_root`) eliminates this entirely — the app sees only its own rootfs.

**Open question:** If user namespaces are unavailable on the target, should we refuse to execute (security-first) or fall back to level 0 with a warning (UX-first)? The answer depends on the deployment context.

See [Isolation](./reference/isolation.md) for the full comparison.

## 5. Syscall filtering — seccomp-bpf

**Attack:** Even inside user namespaces + pivot_root, an attacker could use dangerous syscalls to escalate: load kernel modules (`init_module`), remount filesystems (`mount`), reboot the host (`reboot`), or trace other processes (`ptrace`).

**Defense:** A seccomp-bpf denylist is installed after pivot_root. It blocks ~14 syscalls that have no legitimate use in a packaged web/server app. All other syscalls (networking, file I/O, memory, process creation) are allowed.

```
Blocked: ptrace, mount, umount2, pivot_root, reboot, kexec_load,
         kexec_file_load, init_module, finit_module, delete_module,
         swapon, swapoff, sethostname, setdomainname, acct, nfsservctl
```

**Why denylist, not allowlist:** Python and Node.js use ~150+ distinct syscalls. Maintaining an allowlist that covers every runtime version and platform would break apps unpredictably. A denylist of the clearly dangerous syscalls is sufficient — namespace isolation handles the rest.

**Graceful degradation:** If seccomp is unavailable (kernel without `CONFIG_SECCOMP`, container that blocks `prctl`), the launcher prints a warning and continues without the filter. Isolation is defense-in-depth, not the primary security boundary.

## 6. Payload encryption — AES-256-GCM

**Attack:** A `.ere` file is intercepted at rest (stolen laptop, shared storage, leaked artifact). Without encryption, anyone can extract the embedded application with `tar` after stripping the stub.

**Defense:** Optional AES-256-GCM encryption (v4 format, `--encrypt` flag). A fresh random 32-byte encryption key is generated at build time and stored (hex-encoded) in the binary's metadata next to the ciphertext; the AES key is then derived from it via HKDF-SHA256. The encryption key is deliberately **independent** of the Ed25519 signing seed — the seed is never embedded in the binary, so a key leak does not compromise authenticity.

```bash
# Build with encryption
$ daedalus build my_app/ --key $XDG_DATA_HOME/daedalus/keys/a1b2c3d4.key --encrypt
[daedalus] encrypted: 12.3MB -> 12.3MB (AES-256-GCM)
[daedalus] wrote my_app.ere (12.5MB, signed+encrypted)
```

**Security model:**
- Encryption protects the payload at rest against casual extraction.
- It does not protect against a determined attacker on a machine that must run the decrypted app — the launcher decrypts before exec, and the encryption key is embedded in the metadata for that exact purpose. This is the same fundamental limit as any DRM system.

**Why this is still useful:**
- Prevents `tar` extraction of the embedded app
- Protects against opportunistic theft (stolen build artifacts)
- Adds a layer of defense-in-depth alongside signatures
- The embedded key and ciphertext are both covered by the payload integrity hash and signature — tampering with either breaks the SHA-256/signature check before decryption runs

**Key derivation:**
```
AES_key = HKDF-SHA256(
    key  = random_encryption_key,      # 32 bytes, generated per build
    salt = random_salt,                # 32 bytes, generated per build
    info = "aes-256-gcm-key"           # fixed per algorithm
)
```

Both `random_encryption_key` and `random_salt` are stored in the binary metadata (`meta.crypto`) alongside the ciphertext. With SISR chunking, each chunk additionally gets an independent key via `HKDF(key, salt, chunk_index)` and a unique nonce derived from a per-build base nonce.

**Verification order (non-negotiable):**
```
1. open("/proc/self/exe")               ← self-locate (not argv[0])
2. read footer, validate magic           ← reject early if not .ere
2. ── VERIFY Ed25519 SIGNATURE ──        ← nothing executes before this
2. verify payload SHA-256                ← corruption check (on ciphertext if encrypted)
3. ── DECRYPT PAYLOAD ──                 ← only after steps 3+4
4. atomic extraction (tmp → rename)      ← no TOCTOU window
5. user namespace + pivot_root           ← filesystem isolation (level 2)
6. seccomp filter                        ← syscall filtering (level 2)
7. exec entrypoint
```

Decryption happens after signature verification and SHA-256 integrity check. We decrypt what's already proven authentic. The SHA-256 hash covers the ciphertext, not the plaintext — this ensures integrity verification and signature verification both operate on the same bytes.

Nothing is written to disk before integrity is verified. With signatures enabled, nothing executes before the signature is verified. With encryption, nothing is decrypted before both signature and integrity are verified.