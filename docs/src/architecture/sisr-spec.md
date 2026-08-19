# SISR: Self-Incremental Sovereign Reconstruction

> Status: **Conceptual specification** (Phase 1 — no code, no format change).
> Defines the trust model, invariants, and conceptual ABI for a runtime that
> can rebuild a `.ere` **by itself**, from verified deltas, without any host
> dependency.

SISR ("Self-Incremental Sovereign Reconstruction") turns a `.ere` from a
static container into an autonomous, modular system: the binary can receive a
signed delta, verify it, and re-assemble a new version of itself locally —
without the `erebus` CLI, without a compiler, and without any system runtime.

The purpose of this document is to fix the **absolute invariants**, the
**cryptographic trust model**, and the **conceptual boundaries** *before* any
code is written or any format field is added. It is the contract that
implementation must satisfy, not the implementation.

---

## 1. Motivation

Today a `.ere` is immutable: shipping a fix means rebuilding on a dev machine
and redistributing the whole file. SISR addresses the deployment side of that
loop — the **self-healing runtime** — so a target machine can update itself
from signed deltas and recover from local corruption without the build
toolchain ever being present.

SISR changes what a `.ere` is:

| | Static `.ere` (today) | SISR `.ere` (target) |
|---|---|---|
| Update path | Rebuild + redistribute | Self-rebuild from signed delta |
| Requires toolchain | Only at build time | **Never** |
| Trust anchor | Signature verified once at launch | Signature chain re-verified at every reconstruction |
| Binary identity | Fixed by its bytes | Reconstructed from verified blocks |

Without the invariants in this document, SISR degenerates into a remote-code
execution hole: a malicious delta, a replayed old version, or a tampered local
cache would each be fatal. The invariants below are therefore **non-negotiable
and absolute**.

## 2. Invariants

### I-1. Zero host dependency

A `.ere` that reconstructs itself must never require, on the target machine:

- the `erebus` CLI,
- a compiler (Rust, C, Go),
- any system runtime (node, python, docker),
- any packaging tool (tar, squashfs-tools, zstd).

**Consequence.** All reconstruction machinery — delta decoding, Merkle/CDC
verification, block assembly, block-to-filesystem materialization — must be
embedded in the binary itself, compiled statically (musl, as the launcher
already is) and usable through the same `/proc/self/exe` self-location pattern.

### I-2. Application neutrality

Activating the SISR engine must **never alter the default behavior** of the
hosted application. A user who executes `./app.ere` without any
reconstruction trigger must get exactly today's transparent behavior: launcher
→ verify → cache → exec the server/web app. SISR is a **dormant capability**:
it only engages when explicitly invoked (new binary on disk, explicit update
request), never implicitly.

### I-3. Strict atomicity

Local reconstruction must be **100% atomic**. An interruption at any point
(power loss, `SIGKILL`, crash mid-assembly) must leave the previous binary
perfectly intact and functional.

**Consequence.** The engine never writes in place. It assembles the candidate
next version in a staging location, verifies it end-to-end, then atomically
commits it (rename/swap semantics). A failed or interrupted reconstruction is
invisible: the running and on-disk binaries keep being the last valid version.

---

## 3. Architecture: three responsibilities

A `.ere` is conceptually split into three responsibilities that must remain
cleanly separable:

```
+-----------------------------------------------------------------+
|                      APP.ERE (ELF/PE)                          |
+-----------------------------------------------------------------+
|  1. Entrypoint Launcher  --> Bootstrap + runtime isolation      |
+-----------------------------------------------------------------+
|  2. Embedded SISR Engine --> Verify, fetch & assemble            |
+-----------------------------------------------------------------+
|  3. Payload (SquashFS)   --> Application code & assets           |
+-----------------------------------------------------------------+
```

1. **Entrypoint Launcher** — the existing stub. Reads footer, verifies
   integrity/signature, extracts, execs. Unchanged behavior for normal runs.
2. **Embedded SISR Engine** — dormant code path that can rebuild the binary
   from signed blocks. Never runs during a default launch.
3. **Payload** — the SquashFS application content. SISR reconstructs the
   *payload* (and, when needed, the launcher) as a new assembled `.ere`.

The trust model requires that a `.ere` may only apply a delta or a manifest
**signed by the same Ed25519 public key** that signed the original binary —
or by a valid **delegated key** present in the original header (see
[Trust policy](#5-trust-model)).

---

## 4. Conceptual ABI: Launcher ⟷ SISR Engine

The boundary between the Launcher and the SISR Engine is the **only**
interface the engine is allowed to use. It is kept minimal and fully
in-process (no IPC, no network server, no subprocess):

```
┌────────────────────── Launcher (existing) ──────────────────────┐
│  read footer · verify · extract · exec                          │
├──────────────────────────────────────────────────────────────────┤
│                    SISR Engine (new, dormant)                   │
│                                                                 │
│  self-locate ( /proc/self/exe )                                 │
│  read manifest header                                            │
│  verify trust chain (Ed25519)                                   │
│  verify monotonic anti-rollback index                           │
│  fetch+verify blocks (CDC/Merkle)                                │
│  assemble candidate → verify → atomic commit                    │
└──────────────────────────────────────────────────────────────────┘
```

The engine never shells out and never calls the network at the OS level it
controls end-to-end; fetching uses the same primitives the app already needs
(plain HTTPS), but always **verify-before-assemble**.

## 5. Trust model

### 5.1 Chain of trust

```
original .ere  ── Ed25519 ──►  owner signing key  ── delegate ──►  update key
  (trust anchor)                                                  (signs deltas)
```

- The **trust anchor** is the Ed25519 public key recorded when the binary was
  first produced and signed. It is carried inside the signed region of the
  original file, so it cannot be tampered with without breaking the original
  signature.
- A delta or manifest is accepted only if it is signed by the anchor key
  itself, or by a **delegated key** whose delegation record is signed by the
  anchor (or transitively by an already-validated delegated key).
- **Non-repudiation.** Every accepted reconstruction is backed by a verified
  Ed25519 signature. The engine keeps a signed log of what it applied and when
  (`created` timestamp from the signed manifest), so an update cannot later be
  denied by its originator.

### 5.2 Anti-rollback

- Each signed manifest carries a **monotonic version index**.
- The engine stores the **highest applied index** in the local header/tracking
  state (itself integrity-protected).
- A manifest whose index is **lower or equal** to the stored one is rejected.
  This blocks replay attacks that try to force the binary back to an old,
  vulnerable version.
- The monotonic index is part of the **signed** region, so it cannot be
  lowered by editing the file.

### 5.3 Engine isolation

- The SISR engine runs **before** the application payload is ever exposed to
  untrusted code, or in a dedicated context, so a compromised app cannot
  silently re-run or alter a reconstruction.
- The engine must not be reachable as an attack surface by the running
  application: no open network port, no world-writable IPC path, no
  environment-triggered execution that bypasses signature verification.

### 5.4 TrustPolicy structure

```
TrustPolicy {
  anchor_public_key : Ed25519PublicKey   // trust anchor, from original
  allowed_delegates : [DelegatedKey]     // valid delegation records
  min_format_version: u8                 // never downgrade format
  allow_unsigned    : false              // SISR deltas are ALWAYS signed
}
```

## 6. Structures (conceptual)

The following structures define the data model of SISR. They are a contract
for future implementation, **not** a format change to the current `.ere`
footer (see [Compatibility](#8-compatibility)).

### 6.1 ManifestHeader

```
ManifestHeader {
  magic           : "SISR\x01"            // SISR manifest marker
  version         : u8                    // manifest schema version
  signed_body     : SignedBody            // everything below is signed
}

SignedBody {
  monotonic_index : u64                   // anti-rollback counter
  base_sha256     : [u8; 32]              // hash of the binary this applies to
  target_sha256   : [u8; 32]              // hash of the binary it produces
  created         : DateTime<Utc>         // non-repudiation timestamp
  block_count     : u64                   // number of blocks in this delta
  blocks          : [BlockRef]            // ordered block references
  signature       : Ed25519Signature      // over the whole SignedBody
}
```

### 6.2 BlockDigest

A single content-addressed unit of the delta:

```
BlockDigest {
  block_id        : u64                   // ordinal in the target binary
  source_range    : Range<u64>            // byte range in the target layout
  content_sha256  : [u8; 32]              // hash of the decoded block bytes
  encoding        : "identity" | "zstd"   // how the block is stored in transit
  cdc_boundary    : bool                  // true if this is a CDC split point
}
```

Blocks are addressed by their content hash (Content-Defined Chunking), which
makes the local cache content-addressed and safe: a block is reused from cache
**only** if its `content_sha256` matches what the manifest expects.

### 6.3 Block cache

The engine maintains a content-addressed local cache:

```
~/.cache/erebus/self/{hash}/blocks/<content_sha256>   ← immutable by key
~/.cache/erebus/self/{hash}/applied.index             ← monotonic counter
~/.cache/erebus/self/{hash}/reconstruct/<version>     ← staged candidates
```

A cache block is keyed by its content hash, so **injecting a malicious block**
simply produces a different key and is rejected before assembly. The cache is
advisory — never authoritative — and is always re-validated against the signed
manifest.

## 7. The `SisrRebuilder` contract

The conceptual interface that the SISR engine implements — the contract that
any implementation must satisfy:

```
trait SisrRebuilder {
    fn trust_policy()          -> TrustPolicy;          // static anchor
    fn verify_manifest(m)      -> Result<(), TrustError>; // sig + rollback
    fn fetch_and_verify(m)     -> Result<Vec<Block>, BlockError>; // CDC/Merkle
    fn assemble(m, blocks)     -> Result<Assembly, AssembleError>; // pure
    fn stage(candidate)        -> Result<(), StagingError>; // isolated
    fn commit(candidate)       -> Result<(), CommitError>;  // ATOMIC swap
}
```

Key properties of the contract:

- **`verify_manifest` first**: any failure anywhere in the chain stops the
  reconstruction immediately. No partial assembly is ever observed.
- **`assemble` is pure**: it produces the candidate bytes without touching the
  live binary. The only side-effecting step is `commit`, which is atomic.
- **No external dependency**: all steps are self-contained in the binary
  (musl-static), matching invariant I-1.

## 8. Compatibility

- **No impact on existing `.ere` executables (Phase 1).** Old binaries remain
  plain static SquashFS containers; SISR is a dormant capability that does not
  run on default launch (invariant I-2).
- No existing footer field, layer format, or magic constant is modified. The
  structures in [§6](#6-structures-conceptual) are conceptual contracts for a
  future format evolution; they become real only when the format spec changes
  explicitly, with version constants updated in lockstep (per project rule).

## 9. Security

### 9.1 Anticipated attacks

| Attack | Scenario | Primary defense |
|---|---|---|
| **Replay** | Force the binary to re-assemble onto an old, vulnerable version | Monotonic signed anti-rollback index (§5.2) |
| **Malicious block injection** | Inject a malicious block into the local block cache | Content-addressed cache: wrong hash ⇒ wrong key ⇒ rejected (§6.2) |
| **Manifest spoofing** | Present a forged/edited manifest | Ed25519 chain of trust + non-repudiation (§5.1) |
| **TOCTOU on cache** | Swap cache content between check and use | Cache is re-validated against the signed manifest at assembly; content-addressing makes swaps detectable |
| **Downgrade format** | Try to reconstruct an older, weaker format | `min_format_version` in `TrustPolicy` |
| **Unsigned delta** | Inject an unsigned or partially signed delta | `allow_unsigned = false`; signature required on the whole `SignedBody` |

### 9.2 Non-negotiable verification order

```
1. self-locate ( /proc/self/exe )
2. read manifest header, validate magic
3. ── VERIFY SIGNATURE over SignedBody ──   ← nothing happens before this
4. verify monotonic index > applied index   ← anti-rollback
5. fetch blocks + verify each content_sha256 (CDC/Merkle)
6. ── ASSEMBLE CANDIDATE (pure) ──          ← no live mutation
7. verify candidate hash == target_sha256
8. ── ATOMIC COMMIT ──                      ← rename/swap, interrupt-safe
```

Any failure at any step **aborts the entire chain** — the previous binary is
untouched and still the running one.

## 10. Testing (conceptual)

These are the acceptance properties, not yet concrete tests:

- **Failure-propagation matrix**: for every crypto failure (bad signature,
  replayed index, block hash mismatch, downgraded format, unsigned delta),
  assert the chain stops immediately and the on-disk binary is byte-identical
  to the previous version.
- **Atomicity tests**: kill the engine at every assembly/commit step
  (`SIGKILL`, power-loss simulation); the previous binary must remain
  functional.
- **Responsibility-separation tests**: default launch (no update trigger) must
  behave exactly like today's static container — no SISR code path executes.
- **Anti-rollback tests**: a signed manifest with a stale index is always
  rejected, even with a valid signature.
- **Cache-injection tests**: tampering with any block in the local cache is
  detected and the reconstruction aborts.

## 11. Performance

No CPU/memory/disk impact at this stage — this is a conceptual definition. The
engine is dormant during normal launches (invariant I-2); the performance
budget for reconstruction is not specified here and will be defined when the
implementation design is produced.

---

## References

- [Security model](../security.md) — in particular the new section
  [Chain of Trust for Local Rebuilding](../security.md#1b-chain-of-trust-for-local-rebuilding)
- [`.ere` format](../reference/format.md) — the format this engine will
  eventually reconstruct
- [The Launcher (stub)](../reference/launcher.md) — the existing bootstrap
  responsibility
