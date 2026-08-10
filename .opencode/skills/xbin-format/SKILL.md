---
name: xbin-format
description: x.bin binary format expert - layout, versions, and footer structure
---

## What I do

I understand the x.bin binary format completely. I help with:
- Format version differences (v2, v3, v4, v5)
- Footer structure and magic bytes
- Metadata serialization
- Integrity verification
- SISR delta update extension

## Format layout

```
[stub][payload][metadata][sig?][sisr ext?][footer]
```

- The file starts with a valid ELF executable (the stub), so the kernel runs it directly.
- The payload is a compressed rootfs: `zstd(tar)` (default) or SquashFS (v5).
- The footer is read backwards from EOF. The single source of truth is `xbin-core/src/format.rs`; the launcher and CLI share it.

## Footer structure

All integers little-endian. v3+ is a 92-byte block = an **8-byte `sig_offset` prefix** followed by the **84-byte v2-compatible core**. This keeps v2 launchers working: they read the last 84 bytes and never see the prefix.

```
v2 launcher reads:          v3 launcher reads:
   ┌──────────────┐            ┌──────────────────────┐
   │ 84-byte core │            │ 8B prefix │ 84B core │
   └──────────────┘            └──────────────────────┘
   EOF-84 → EOF                EOF-92 → EOF
```

### Footer core (84 bytes)

| Field            | Offset | Type  | Size | Description                                  |
|------------------|--------|-------|------|----------------------------------------------|
| `magic`          | 0      | bytes | 5    | `"XBIN\x01"`                                 |
| `format_version` | 5      | u8    | 1    | format version (2, 3, 4, or 5)               |
| `arch`           | 6      | u8    | 1    | `0x01`=x86_64, `0x02`=aarch64                |
| `flags`          | 7      | u8    | 1    | bit0=signed, bit1=encrypted, bit2=SISR       |
| `payload_offset` | 8      | u64   | 8    | absolute offset of payload                   |
| `payload_csize`  | 16     | u64   | 8    | compressed size of all layers                |
| `payload_usize`  | 24     | u64   | 8    | v2/v3: unused; v4/v5: crypto_suite (0=none, 1=AES-256-GCM) |
| `payload_sha256` | 32     | bytes | 32   | SHA-256(payload ‖ metadata)                  |
| `meta_offset`    | 64     | u64   | 8    | absolute offset of metadata                  |
| `meta_size`      | 72     | u64   | 8    | metadata size in bytes                       |
| `footer_magic`   | 80     | u32   | 4    | `0xBEEFCAFE` end sentinel                    |

### v3+ prefix (8 bytes)

| Field        | Offset | Type | Size | Description |
|--------------|--------|------|------|-------------|
| `sig_offset` | 0      | u64  | 8    | absolute offset of signature block (0 if unsigned) |

### Constants

- Footer magic: `0xBEEF_CAFE`
- Format magic: `XBIN\x01`
- `V2_FOOTER_SIZE = 84`, `V3_FOOTER_SIZE = 92`
- `SIG_BLOCK_SIZE = 68` (4-byte size field + 64-byte Ed25519 sig)
- `SISR_FOOTER_EXT_SIZE = 110` — fixed SISR access block placed immediately before the footer

## Format versions

- **v2**: Plain layered payload, no signing/encryption (footer 84B)
- **v3**: Ed25519 signed (92B footer with `sig_offset` prefix)
- **v4**: AES-256-GCM encrypted (`crypto_suite` in `payload_usize`)
- **v5**: SquashFS layer support (`"payload_format": "squashfs"` in metadata)

The launcher rejects files with a version higher than it understands.

## Flags

- `FLAG_SIGNED = 0x01`
- `FLAG_ENCRYPTED = 0x02`
- `FLAG_SISR = 0x04`

## Integrity hash

```rust
let hash = Sha256::digest(payload || meta_bytes);
```

Computed at build time, stored in `payload_sha256`, recomputed and compared
**before** extraction at runtime. For signed files, the same hash is what the
Ed25519 signature covers (verification happens before decryption).

## Signature block (68 bytes)

Inserted between metadata and footer when `flags & FLAG_SIGNED`:

| Field      | Type  | Size | Description              |
|------------|-------|------|--------------------------|
| `sig_size` | u32le | 4    | always 64 (Ed25519 sig)  |
| `signature`| bytes | 64   | Ed25519 signature        |

`footer.sig_offset` points at the start of this block.

## SISR (Self-Incremental Sovereign Reconstruction)

- `SISR_FOOTER_EXT_SIZE = 110` bytes of access block before the footer
- `FLAG_SISR` set in footer flags
- Delta manifest describes chunked rootfs + Merkle tree; the stub reconstructs from a local reuse index or fetches delta from remote
- See `docs/src/spec/xbin-format-v2.md` and `docs/src/architecture/sisr-spec.md`

## Key rules

1. Never change magic bytes
2. Never change version constants without updating `xbin-core/src/format.rs`
3. Always update metadata when adding new fields
4. Preserve backward compatibility — a v2 launcher must never break on a v3+ file (this is why `sig_offset` is a prefix, not an appended field)
5. Test all format versions
6. Do NOT invent a new format to "fix" an old one — extend the existing one (see XKCD 927 in `docs/src/concepts/positioning.md`)

## Files to modify

- `xbin-core/src/format.rs`: Format definitions (single source of truth)
- `xbin-core/src/metadata.rs`: Metadata structure
- `stub/src/main.rs`: Footer reading
- `docs/src/reference/format.md`: Human-readable format spec
