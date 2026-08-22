# Delta Manifest Format

> **Status: conceptual spec.** No format has been frozen yet. This document
> defines the structure that a SISR update is described by, and how it
> applies to SquashFS images. It is the contract for a future format
> evolution, not a change to the current `.ere` format.

An incremental update is driven by a **manifest** — a small, signed document
that answers three questions:

1. *What* is this update, and for *which* binary?
2. *What changed*, and *where* (the delta / block list)?
3. *How do I verify* each part (hashes, signatures, rollback guard)?

Everything else is derived from the answer.

## 1. Data model

### 1.1 Manifest header

```
ManifestHeader {
  magic            : "SISR\x01"
  version          : u8
  signed_body      : SignedBody
}
```

`version` is the manifest schema version. The launcher rejects manifests with
a schema it does not understand (same rule as `.ere` format versions).

### 1.2 Signed body — the trust boundary

The whole update decision lives in the **signed body**. Nothing outside it is
trusted:

```
SignedBody {
  monotonic_index  : u64                // anti-rollback counter
  base_sha256      : [u8; 32]           // hash of the binary being updated
  target_sha256    : [u8; 32]           // hash of the binary being produced
  created          : DateTime<Utc>      // non-repudiation timestamp
  base_version     : string             // e.g. "1.0.0"
  target_version   : string             // e.g. "1.1.0"
  blocks           : [BlockEntry]       // ordered block references
  signature        : Ed25519Signature   // over the entire SignedBody
}
```

Guarantees:

- **Integrity + non-repudiation.** The Ed25519 signature covers every byte of
  the decision (index, hashes, versions, block list). Nothing can be edited
  without invalidating it.
- **Anti-rollback.** `monotonic_index` is part of the signed body, so a stale
  (replayed) update cannot be relabeled as fresh. The engine rejects any
  index ≤ the highest one it has applied.
- **Exact applicability.** `base_sha256` pins the update to a specific source
  binary. A manifest downloaded for `v1.0` is refused by a `v1.2` binary.
- **Exact outcome.** `target_sha256` lets the engine verify the rebuilt
  binary byte-for-byte before committing it.

### 1.3 Block entry — one content-addressed chunk

```
BlockEntry {
  block_id        : u64
  target_range    : Range<u64>          // byte range in the target layout
  content_sha256  : [u8; 32]            // hash of the decoded block bytes
  encoding        : "identity" | "zstd" // how the block is stored in transit
  source_uri      : string              // where to fetch the encoded block
}
```

Blocks are **content-addressed** (`content_sha256`). This is what makes the
local cache safe: a block is reused from the cache only if its hash matches —
a tampered block has a different hash, hence a different key, hence is never
used.

## 2. SquashFS and content-defined chunking

The payload of a `.ere` is a **SquashFS image** (or a zstd-tar layer). The
engine updates it by reconstructing the image block by block. Two facts shape
the design:

1. **SquashFS is a single, usually compressed, file.** You cannot
   "patch a file in place" inside a compressed image — a one-byte change
   ripples through the compression window.
2. **Most of the image is unchanged between versions.** Only the app code
   layer changes; the runtime layer is identical.

SISR therefore does not store whole-image diffs. It splits the target image
into **content-defined chunks (CDC)** and lets the manifest reference only
the ones that changed:

```
v1.0 image  chunk0  chunk1  chunk2  chunk3  chunk4        (5 chunks)
                 │        │                    │
                 └────────┴────────────────────┘  unchanged → reuse locally
                 ⬇
v1.1 image  chunk0  chunk1  chunk5  chunk3  chunk4        (chunk2 → chunk5)
                              │
                              └─ only this chunk is downloaded & verified
```

CDC boundaries are derived from the content itself (e.g. rolling hash hitting
a target entropy), not from fixed offsets — so an edit in the middle of the
app keeps most chunk boundaries stable and most chunks reusable.

### Why per-block SHA-256 is not enough (and how it is completed)

Per-block SHA-256 verifies each chunk in isolation. SISR additionally binds
the **order and identity** of the chunks with a Merkle-style commitment in
the manifest:

```
target image bytes
   └── leaf: SHA-256(decoded chunk)          ← one per BlockEntry
          └── levels: SHA-256(left ‖ right)  ← hash pairs up the tree
                 └── root = target_sha256     ← signed in the manifest
```

Each leaf corresponds to a `BlockEntry.content_sha256`; internal nodes are
hashes of child hashes; the root is the signed `target_sha256`. Reconstructing
the image and recomputing the root proves, in one step, that **every block is
present, in the right order, and untouched** — including blocks that were
reused from the local cache rather than downloaded.

### The non-negotiable verification order

```
1. self-locate ( /proc/self/exe )
2. read manifest header, validate magic + schema version
3. ── VERIFY Ed25519 SIGNATURE over SignedBody ──   ← nothing before this
4. verify monotonic_index > highest applied index   ← anti-rollback
5. verify base_sha256 == hash(current binary)       ← exact applicability
6. fetch missing blocks, verify each content_sha256
7. reuse cached blocks (verified by content hash)
8. ── ASSEMBLE candidate image (pure) ──
9. verify Merkle root == target_sha256
10. ── ATOMIC COMMIT ──                              ← interrupt-safe swap
```

Any failure at any step aborts the whole update. The previous binary is
untouched.

## 3. Example manifest

```json
{
  "magic": "SISR\x01",
  "version": 1,
  "signed_body": {
    "monotonic_index": 42,
    "base_sha256": "c0a1...",
    "target_sha256": "9e61...",
    "created": "2026-08-02T12:00:00Z",
    "base_version": "1.0.0",
    "target_version": "1.1.0",
    "blocks": [
      {"block_id": 0, "target_range": [0, 4194304], "content_sha256": "a1...",
       "encoding": "identity",  "source_uri": "https://updates.example.com/v1.1/b0"},
      {"block_id": 1, "target_range": [4194304, 8388608], "content_sha256": "b2...",
       "encoding": "identity",  "source_uri": "https://updates.example.com/v1.1/b1"},
      {"block_id": 2, "target_range": [8388608, 12582912], "content_sha256": "c3...",
       "encoding": "zstd",      "source_uri": "https://updates.example.com/v1.1/b2"},
      {"block_id": 3, "target_range": [12582912, 16777216], "content_sha256": "d4...",
       "encoding": "identity",  "source_uri": "https://updates.example.com/v1.1/b3"}
    ],
    "signature": "base64(...)"
  }
}
```

Note that the manifest references the **target** layout. Blocks that are
identical to the current binary (same content hash) are reused locally — the
engine only fetches the `source_uri` of blocks it does not already have in its
content-addressed cache.

## 4. Local cache layout

The engine keeps a content-addressed cache, so reused blocks are validated by
construction:

```
~/.cache/daedalus/self/{hash}/
  blocks/<content_sha256>     ← immutable by key; tampering ⇒ different key
  applied.index               ← highest applied monotonic index (protected)
  reconstruct/<target_hash>/  ← staged candidate assembly (never live)
```

- The cache is **advisory, never authoritative** — every block is re-verified
  against the signed manifest at assembly time.
- `applied.index` is what makes replay impossible across runs: it records the
  highest monotonic index and is itself integrity-protected.

## 5. Relationship to the `.ere` format

This manifest is an **update artifact**, not a new `.ere` layout:

| | `.ere` format (today) | SISR manifest (future) |
|---|---|---|
| Role | describes a binary | describes how to build the *next* binary |
| Location | embedded at end of the file | external (fetched from a remote) |
| Signed | Ed25519 (optional today) | Ed25519 (always) |
| Versioning | format version in footer | schema version in header + monotonic index |

The manifest's `target_sha256` is exactly the hash the rebuilt `.ere`'s
footer must contain. SISR reconstructs a **valid, standard `.ere`** — the
result is indistinguishable from one produced by `daedalus build`.

> This page describes the *external update artifact* (JSON). The `.ere`
> file additionally embeds a compact **binary** chunk index — the same block
> list serialized as `DeltaManifest` — so the runtime can verify the embedded
> content before it is ever assembled. See
> [`.ere` Format v2 — SISR extension](./daedalus-format-v2.md).

## References

- [SISR overview](../concepts/sisr-overview.md) — the two daedalus models and the
  full reconstruction flow.
- [SISR conceptual specification](../architecture/sisr-spec.md) — invariants
  and trust model (this spec derives from it).
- [Chain of Trust for Local Rebuilding](../security.md#1b-chain-of-trust-for-local-rebuilding)
