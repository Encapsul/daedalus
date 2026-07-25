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

## Format layout

```
[stub][payload][metadata][footer]
```

### Footer structure

```rust
struct Footer {
    magic: u32,           // 0xBEEF_CAFE
    version: u32,         // 2, 3, 4, or 5
    metadata_offset: u64, // offset to metadata
    metadata_size: u64,   // size of metadata
    integrity_hash: [u8; 32], // SHA-256(payload || meta_bytes)
}
```

### Format magic

- Footer magic: `0xBEEF_CAFE`
- Format magic: `XBIN\x01`

### Format versions

- **v2**: Plain (no encryption/signing)
- **v3**: Signed (Ed25519)
- **v4**: Encrypted (AES-256-GCM)
- **v5**: Squashfs compressed

## Metadata structure

```rust
struct Metadata {
    version: u32,
    runtime: Runtime,
    entrypoint: String,
    env_vars: HashMap<String, String>,
    created_at: u64,
    // ... other fields
}
```

## Integrity verification

```rust
// Computed at build time
let hash = Sha256::digest(payload || meta_bytes);

// Verified at runtime
if computed_hash != footer.integrity_hash {
    return Err("Integrity check failed");
}
```

## Key rules

1. Never change magic bytes
2. Never change version constants without updating format.rs
3. Always update metadata when adding new fields
4. Preserve backward compatibility
5. Test all format versions

## Files to modify

- `xbin-core/src/format.rs`: Format definitions
- `xbin-core/src/metadata.rs`: Metadata structure
- `stub/src/main.rs`: Footer reading
