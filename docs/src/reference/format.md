# `.ere` Format

A `.ere` file is a **valid ELF executable** with a compressed payload,
JSON metadata, and a **footer** at the very end of the file. The launcher
reads itself via `/proc/self/exe`, seeks to the end, reads the footer, and
finds everything else from the offsets stored there.

```
offset 0    ┌─────────────────────────┐
            │   ELF launcher (musl)   │  ← kernel executes this
            ├─────────────────────────┤
payload_off │   payload = zstd(tar)   │  ← compressed rootfs
            ├─────────────────────────┤
meta_off    │   metadata (JSON utf8)  │  ← entrypoint, env, runtime...
            ├─────────────────────────┤
EOF - 92    │   FOOTER (92 bytes)     │  ← read backwards by launcher
            └─────────────────────────┘
```

The format is versioned (`format_version` in the footer). v1 uses a single
monolithic payload; v2 splits the payload into layers; v3 adds Ed25519
signatures; v4 adds AES-256-GCM encryption; v5 adds SquashFS layer support.
The launcher reads all versions.

## Footer layout (v3/v4/v5, 92 bytes)

All integers are little-endian. The v3 footer is a 92-byte block: an 8-byte
prefix (`sig_offset`) followed by the 84-byte v2-compatible core.

| Field            | Offset | Type  | Size | Description                                  |
|------------------|--------|-------|------|----------------------------------------------|
| `sig_offset`     | 0      | u64   | 8    | absolute offset of signature block (0 if unsigned) |
| `magic`          | 8      | bytes | 5    | `"ERE\x01"`                                 |
| `format_version` | 13     | u8    | 1    | format version (3, 4, or 5)                  |
| `arch`           | 14     | u8    | 1    | `0x01`=x86_64, `0x02`=aarch64                |
| `flags`          | 15     | u8    | 1    | bit0=signed, bit1=encrypted                 |
| `payload_offset` | 16     | u64   | 8    | absolute offset of payload                   |
| `payload_csize`  | 24     | u64   | 8    | compressed size of all layers                |
| `payload_usize`  | 32     | u64   | 8    | v2/v3: unused; v4/v5: crypto_suite (0=none, 1=AES-256-GCM) |
| `payload_sha256` | 40     | bytes | 32   | `SHA-256(payload ‖ metadata)`                |
| `meta_offset`    | 72     | u64   | 8    | absolute offset of metadata                  |
| `meta_size`      | 80     | u64   | 8    | metadata size in bytes                       |
| `footer_magic`   | 88     | u32   | 4    | `0xBEEFCAFE` end sentinel                    |

The spec is implemented in `daedalus-core/src/format.rs` — the single source of
truth, shared by the launcher (stub) and the CLI. There is no separate
`stub/src/format.rs`.

### Design decision: footer at end, not header at start

**Constraint:** The Linux kernel requires ELF magic (`\x7fELF`) at offset 0
to execute the file. We cannot put our own bytes at the start without
breaking `chmod +x && ./my_app.ere`.

**Options considered:**
1. Custom header before ELF — rejected. The kernel would refuse to execute
   the file. `binfmt_misc` could work but requires root on every target
   machine.
2. Embed metadata inside ELF sections — rejected. The kernel loads ELF
   sections into memory; our metadata would consume address space and
   confuse debuggers.
3. Footer at end of file — chosen. Used by `makeself`, AppImage, and
   self-extracting `.exe`. The launcher opens itself via `/proc/self/exe`,
   seeks from the end, reads the fixed-size footer, and finds everything
   from the stored offsets.

### Design decision: v3 8-byte prefix for signatures

**Constraint:** Ed25519 signatures need to be stored between metadata and
the footer. But the footer is at a fixed position (EOF-84 for v2), and
v2 launchers read exactly 84 bytes from the end. We cannot change the
footer size without breaking backward compatibility.

**Options considered:**
1. Grow the footer to 92 bytes (add `sig_offset` field) — rejected. A v2
   launcher reading 84 bytes would see truncated data and likely crash or
   silently misparse.
2. Append the signature block after the footer — rejected. The launcher
   reads backwards from EOF; data after the footer is invisible to it.
3. 8-byte prefix before the 84-byte core — chosen. The last 92 bytes are:
   `[8-byte sig_offset][84-byte v2 core]`. A v2 launcher reads the last 84
   bytes and sees valid v2 data (the prefix is invisible). A v3 launcher
   reads 92 bytes and picks up `sig_offset` from the prefix. No breaking
   change.

```
v2 launcher reads:          v3 launcher reads:
   ┌──────────────┐            ┌──────────────────────┐
   │ 84-byte core │            │ 8B prefix │ 84B core │
   └──────────────┘            └──────────────────────┘
   EOF-84 → EOF                EOF-92 → EOF
```

## Integrity hash

The `payload_sha256` field stores:

```
SHA-256(compressed_payload_bytes ‖ metadata_json_bytes)
```

The launcher recomputes this hash on every cold start and compares it to the
stored value **before** extracting anything. On mismatch: `exit(1)`, nothing
written to disk.

For signed files (v3+), the digest the signature covers also includes the
footer's full on-disk form (the 8-byte `sig_offset` prefix + the 84-byte core):

```
Ed25519_sign(SHA-256(payload ‖ metadata ‖ footer_bytes), private_key)
```

The footer must be covered because it decides whether the signature is ever
consulted: a signature over `payload ‖ metadata` alone would let an attacker
downgrade the file to v2, clear `FLAG_SIGNED`, and recompute the SHA-256 —
the signature would be silently skipped. The launcher also rejects
inconsistent states (sig block without the flag, or the flag without a block)
and v2 files carrying a leftover signature block.

See [Security](../security.md) for why SHA-256 alone is insufficient.

## Signature block (v3, 68 bytes)

Inserted between metadata and footer when `flags & FLAG_SIGNED`:

| Field      | Type  | Size | Description              |
|------------|-------|------|--------------------------|
| `sig_size` | u32le | 4    | always 64 (Ed25519 sig)  |
| `signature`| bytes | 64   | Ed25519 signature        |

The footer's `sig_offset` field points to the start of this block.

## JSON metadata

```json
{
  "name": "hello-web",
  "daedalus_version": "0.1.0",
  "created": "2026-06-23T12:00:00Z",
  "runtime": "python",
  "isolation": 0,
  "entrypoint": ["/usr/bin/python3.12", "/app/app.py"],
  "env": { "PYTHONUNBUFFERED": "1" },
  "layers": [...]
}
```

- `entrypoint`: argv executed by the launcher. Paths are relative to the
  rootfs — the launcher resolves them to real cache paths at exec time.
- `env`: additional variables. The launcher injects `LD_LIBRARY_PATH`
  separately and resolves `${ROOTFS}` tokens to the real cache path.
- `isolation`: 0 = `LD_LIBRARY_PATH`, 1 = chroot (skipped), 2 = user
  namespaces + `pivot_root`.
- `layers`: array of compressed layer objects (v2+ only).

## Layers (v2+)

The payload is a sequence of **layers**, each an independent compressed blob.
Layers stack at extraction (later layers overwrite earlier ones):

```
[ stub ][ runtime layer ][ app layer ][ metadata ][ sig? ][ footer ]
         ^                 ^
         python+stdlib+.so app code + site-packages
         stable            volatile
```

**v2–v4** uses `zstd(tar)` blobs. **v5** uses SquashFS images (better
compression ratio). The layer format is indicated by the `"payload_format"`
field in metadata: `"zstd-tar"` (default) or `"squashfs"`.

The layer table lives in the JSON metadata:

```json
"layers": [
  {"kind": "runtime", "offset": 614960, "csize": 6710886, "usize": 26953728,
   "sha256": "168a7279b815..."},
  {"kind": "app",     "offset": 7325846, "csize": 12044,   "usize": 204800,
   "sha256": "9e61cd65ed9e..."}
]
```

`offset` is the absolute byte offset in the file. Each layer's SHA-256 (of
the compressed blob) is a **stable cache key** — if the content doesn't
change, its extraction is reusable.

### Why layers: incremental rebuild

The runtime layer (interpreter + stdlib + `.so`) is independent of app code.
Editing `app.py` doesn't change it. On rebuild, the builder reuses it from
the build cache (`~/.cache/daedalus/build/`) — no recompression. Only the app
layer is rebuilt.

```bash
# First build: ~25s (compressing runtime layer, ~54 MB)
$ daedalus build ./my_app
[daedalus] wrote my_app.ere (7.1MB) in 25.1s

# Rebuild after code change: ~1s (runtime layer reused)
$ daedalus build ./my_app
[daedalus] wrote my_app.ere (7.1MB) in 1.2s
```

Two apps sharing the same runtime share the same runtime layer in the build
cache — the second app also builds in ~1 s.

## Version evolution

The footer is versioned (`format_version`). A launcher rejects a file with
a version higher than it understands:

```bash
$ ./old-daedalus new-format.ere
[daedalus] error: unsupported .ere format version (binary newer than launcher)
```

| Version | Changes                                            |
|---------|----------------------------------------------------|
| v1      | Monolithic zstd(tar) payload                       |
| v2      | Layered payload (runtime + app), incremental rebuild |
| v3      | Ed25519 signatures (92-byte footer with sig_offset) |
| v4      | AES-256-GCM payload encryption (crypto_suite in footer) |
| v5      | SquashFS layer support (payload_format in metadata) |

Reserved fields (`flags`, `sig_offset`) allow extension without breaking
compatibility. Ed25519 signatures are inserted between metadata and footer
with a `flags` bit and a dedicated offset — v2 files remain readable.
