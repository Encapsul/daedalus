# `.xbin` Format

A `.xbin` file is a **valid ELF executable** with a payload, metadata, and
a **footer** appended to the very end of the file.

The format is **versioned** (`format_version` in the footer). v1 uses a
single monolithic payload; **v2** (current) splits the payload into **layers**
(see below). The launcher reads both.

## Why a footer at the end, not a header at the beginning?

This is the most important design decision in the format.

The Linux kernel requires the ELF magic (`\x7fELF`) at **offset 0** to
execute the file. We **cannot** put our own magic bytes at the start without
breaking executability.

The solution (used by `makeself`, AppImage, self-extracting `.exe`):

- the **ELF launcher** occupies the start of the file — the kernel executes it;
- our data is **appended after**;
- a **fixed-size footer** is placed at the very end of the file;
- at startup, the launcher opens itself via `/proc/self/exe`, `seek`s from
  the end, reads the footer, and finds the offsets for everything else.

```
offset 0    ┌─────────────────────────┐
            │   ELF launcher (musl)   │  ← executed by the kernel
            ├─────────────────────────┤
payload_off │   payload = zstd(tar)   │  ← compressed rootfs
            ├─────────────────────────┤
meta_off    │   metadata (JSON utf8)  │  ← entrypoint, env, runtime...
            ├─────────────────────────┤
EOF - 84    │   FOOTER (84 bytes)     │  ← read backwards by the launcher
            └─────────────────────────┘
```

## Footer (84 bytes, fixed)

All integers are little-endian.

| Field            | Type  | Size | Description                             |
|------------------|-------|------|-----------------------------------------|
| `magic`          | bytes | 5    | `"XBIN\x01"`                            |
| `format_version` | u8    | 1    | format version (= 1)                    |
| `arch`           | u8    | 1    | `0x01`=x86_64, `0x02`=aarch64           |
| `flags`          | u8    | 1    | bit0=signed, bit1=encrypted (0 in MVP)  |
| `payload_offset` | u64   | 8    | absolute offset of payload              |
| `payload_csize`  | u64   | 8    | compressed size                         |
| `payload_usize`  | u64   | 8    | uncompressed size (tar)                 |
| `payload_sha256` | bytes | 32   | SHA-256 of compressed payload           |
| `meta_offset`    | u64   | 8    | absolute offset of metadata             |
| `meta_size`      | u64   | 8    | metadata size                           |
| `footer_magic`   | u32   | 4    | `0xBEEFCAFE` — end sentinel             |

Total: **84 bytes**. The spec is shared between `stub/src/format.rs` (read)
and `cli/xbin/format.py` (write) — both **must** stay synchronized.

## JSON metadata

```json
{
  "name": "hello-web",
  "xbin_version": "0.1.0",
  "created": "2026-06-23T12:00:00Z",
  "runtime": "python",
  "isolation": 0,
  "entrypoint": ["/usr/bin/python3.12", "/app/app.py"],
  "env": { "PYTHONUNBUFFERED": "1", "PYTHONDONTWRITEBYTECODE": "1" },
  "layers": [...]
}
```

- `entrypoint`: argv executed by the launcher. Absolute paths are
  **relative to the rootfs** (the launcher prefixes them with the real cache
  path, or resolves them after pivot_root).
- `env`: additional variables (the launcher injects `LD_LIBRARY_PATH` separately).
- `isolation`: 0 = `LD_LIBRARY_PATH`, 1 = chroot (skipped), 2 = user namespaces.
- `layers`: table of compressed layer blobs (v2+ only).

## Layers (v2)

In v2, the payload is a sequence of **layers**, each an independent
`zstd(tar)` blob, stacked at extraction (later layers overwrite earlier
ones — similar to Docker layers):

```
[ stub ][ runtime layer ][ app layer ][ metadata ][ footer ]
         ^                 ^
         python+stdlib+.so app code + site-packages
         stable            volatile
```

The **footer** keeps the same 84-byte structure; its semantics adapt:

| Field            | v1                    | v2                                  |
|------------------|-----------------------|-------------------------------------|
| `payload_offset` | start of payload      | start of **layer region**           |
| `payload_csize`  | payload size          | total compressed size of all layers |
| `payload_usize`  | uncompressed size     | unused (per-layer sizes in meta)    |
| `payload_sha256` | SHA-256(payload)      | SHA-256(**layers ‖ metadata**)      |

The **layer table** lives in the JSON metadata:

```json
"layers": [
  {"kind": "runtime", "offset": 614960, "csize": 6710886, "usize": 26953728,
   "sha256": "168a7279b815..."},
  {"kind": "app",     "offset": 7325846, "csize": 12044,   "usize": 204800,
   "sha256": "9e61cd65ed9e..."}
]
```

`offset` is the **absolute** offset in the file. Each layer's SHA-256 (of the
compressed blob) serves as a **stable cache key**: as long as a layer's
content doesn't change, its extraction is reusable.

### Why layers: incremental rebuild

This is the reason v2 exists. The **runtime** layer (interpreter + stdlib +
`.so`) is **independent of app code**: editing `app.py` doesn't change it.
On rebuild, the builder **reuses** it from its build cache
(`~/.cache/xbin/build/`) — no recompression. Only the small **app** layer
is rebuilt.

```
initial build  : ~25 s  (compressing runtime layer, ~54 MB)
rebuild (code)  : ~1 s   (runtime layer reused, only app recompresses)
```

Bonus: two apps sharing the same runtime (same interpreter + libs) share the
**same runtime layer** in the build cache — the second app also builds in
~1 s. See [The builder](./builder.md).

## Why the format survives evolution

- The footer is **versioned** (`format_version`). A launcher gracefully
  rejects a file with a version higher than it understands.
- Reserved fields (`flags`, and the signature block added in Phase 2) allow
  extension without breaking compatibility.
- Ed25519 signatures are inserted between `metadata` and `footer`, with a
  `flags` bit and a dedicated offset in the v2 footer — v1 files remain
  readable.
