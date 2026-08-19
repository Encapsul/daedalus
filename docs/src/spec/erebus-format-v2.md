# `.ere` Format v2 — SISR extension

> Status: **implemented** (`erebus-core` `sisr_header` + `manifest` modules).
> Specifies the `.ere` layout enriched with the `SISR` header and the
> embedded delta manifest, and how it stays byte-for-byte backward
> compatible with existing decoders.

The stock `.ere` layout `[stub][payload][metadata][footer]` is unchanged.
`SISR` adds two blocks between the app metadata and the standard footer, and
signals their presence with one spare bit of the footer `flags` byte.

## Physical layout

```
+------------------------------------------------------------------+
| Executable Header & Embedded Runtime (ELF/PE)                    |
+------------------------------------------------------------------+
| Application Payload (SquashFS Compressed Data)                   |
+------------------------------------------------------------------+
| App metadata (JSON)                                              |
+------------------------------------------------------------------+
| Signature block (v3+, optional, unchanged)                       |
+------------------------------------------------------------------+
| SISR Section (DeltaManifest: chunk index)                       |
+------------------------------------------------------------------+
| SISR footer extension (110 bytes, fixed)                        |
+------------------------------------------------------------------+
| Standard erebus Footer (84 or 92 bytes, EREBUS_MAGIC at EOF)        |
+------------------------------------------------------------------+
```

Offsets within the standard footer (`payload_offset`, `meta_offset`,
`sig_offset`) are absolute and therefore unaffected by the insertion.

## Backward compatibility

- `EREBUS_MAGIC` (`0x5842494E` + `\x01`) stays at the exact end of the file,
  and the standard footer is byte-for-byte identical with or without `SISR`.
- Legacy decoders read the footer backwards from EOF; the `SISR` blocks sit
  *before* it and are never seen.
- Presence is signaled by the `FLAG_SISR` bit (`0x04`) of the footer `flags`
  byte. Existing readers that only test `FLAG_SIGNED` / `FLAG_ENCRYPTED` are
  unaffected; a legacy file without the bit decodes transparently.
- A new reader locates the extension at
  `file_len - footer_size - 110`, where `footer_size` is 84 (v2) or 92
  (v3+), from `Footer::footer_size()`.

## SISR footer extension (110 bytes, little-endian)

| offset | size | field                | meaning                                     |
|--------|------|----------------------|---------------------------------------------|
| 0      | 2    | `sisr_version`       | extension schema version (`1`); `0` = absent |
| 2      | 8    | `chunk_table_offset` | absolute file offset of the DeltaManifest   |
| 10     | 4    | `chunk_table_len`    | byte length of the DeltaManifest            |
| 14     | 32   | `merkle_root`        | Merkle root over the payload chunk hashes   |
| 46     | 64   | `signature`          | Ed25519 signature over `merkle_root ‖ manifest` |

Serialized by `SisrFooterExt::pack()` / `SisrFooterExt::parse()`. The
signature is computed over the concatenation of the 32-byte Merkle root and
the serialized `DeltaManifest`, so the extension and the chunk table are bound
together: any alteration of either invalidates the signature.

## DeltaManifest (the SISR Section)

| offset | size  | field         |
|--------|-------|---------------|
| 0      | 4     | magic `XBMD`  |
| 4      | 1     | version (`1`) |
| 5      | 3     | reserved (zero) |
| 8      | 4     | `chunk_count` |
| 12     | 8     | `payload_len` |
| 20     | 36·n  | `ChunkEntry` table |

`ChunkEntry` = `hash [u8; 32]` + `length u32`, little-endian. Chunks are in
payload order; each is addressed by content, so reuse and tamper detection
follow from the hash. See the [delta manifest format](./delta-manifest-format.md)
for how the chunk list drives an incremental update.

## Remote manifest (`<name>.ere.manifest`)

The builder writes a self-contained, signed copy of the manifest next to the
binary:

```
| magic "XBMR" (4) | version (1) | reserved (3) | merkle_root (32) | signature (64) | DeltaManifest |
```

The signature is over `merkle_root ‖ DeltaManifest` (identical to the embedded
header), so the remote file can be served over HTTPS / a package registry and
verified offline without touching the `.ere`. `RemoteManifest::verify_signature`
and `verify_merkle` check both bindings.

## Security

Strict bounds checking everywhere a hostile file could trigger OOB reads or
over-allocation:

- `chunk_table_offset + chunk_table_len` is checked with checked arithmetic
  and must end before the SISR extension (never in the footer or the
  extension itself).
- The manifest buffer length must equal `20 + 36 × chunk_count` exactly;
  count-derived sizes are computed with checked arithmetic **before** the
  chunk vector is allocated, so a forged `chunk_count` fails without
  allocating or over-reading.
- Unknown schema versions are rejected (same rule as `.ere` format
  versions).

## Performance

Fixed overhead: **110 bytes** for the extension. The manifest adds
`20 + 36n` bytes for `n` chunks. A 100-chunk app therefore adds ~3.7 KiB of
`SISR` header metadata — under the 4 KiB budget. The extension carries the
Merkle root and signature so the runtime can pre-check integrity without
parsing the manifest.

## Related

- [SISR conceptual specification](../architecture/sisr-spec.md)
- [Delta manifest format](./delta-manifest-format.md)
- [Internal crates](../architecture/internal-crates.md)
