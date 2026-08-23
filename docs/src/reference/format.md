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

---

# Universal Polyglot Binary Format

A **universal** `.daedalus` file bundles multiple architecture-specific
`.ere` slices behind a polyglot shell-script launcher. At runtime, the
launcher detects the host OS/architecture via `uname`, extracts the
matching slice to a temp file, and `exec`s it. Each slice is a complete,
independent `.ere` binary.

## Motivation

A single self-contained binary that runs unmodified on:

- **Linux**: x86_64, aarch64, riscv64
- **macOS**: x86_64 (Intel), arm64 (Apple Silicon)

is achieved by concatenating self-extracting `.ere` slices and prepending
a POSIX-compliant shell script that knows where each slice lives.

## Layout

```
offset 0        ┌──────────────────────────┐
                │  shell-script launcher    │  64 KiB (fixed), null-padded
                │  (polyglot with ELF header│                      )
                ├──────────────────────────┤
slice_1_off     │  linux-x86_64 .ere slice │
                ├──────────────────────────┤
slice_2_off     │  linux-aarch64 .ere slice│
                ├──────────────────────────┤
  ...           │  ...                     │
                ├──────────────────────────┤
manifest_off    │  JSON manifest (4 KiB)   │  fixed-size, null-padded
                ├──────────────────────────┤
EOF - 26        │  universal footer (26 B) │
                └──────────────────────────┘
```

### Polyglot trick

The first 64 KiB is simultaneously:

1. **A valid shell script** — the kernel's `#!` handler treats `#!/bin/sh`
   at offset 0 as a directive to invoke `/bin/sh`. The shell reads and
   executes the script, which extracts the matching slice and `exec`s it.
2. **A valid ELF/PE/Mach-O file** (when the slice is extracted alone) —
   each slice is a standalone `.ere` binary that the kernel can load
   directly.

The shell script occupies exactly `64 * 1024` bytes. Bytes after the
script's `exec` line are unused padding (zeros), so the shell never
parses them. When a slice is extracted to a temp file, the file begins
with the slice's own binary header (ELF magic, PE, or Mach-O magic).

## Universal footer (26 bytes)

```
Offset  Type   Field             Description
0       u32le  magic             0xBEEFCABE (little-endian)
4       u32le  num_slices        Number of ArchSlices
8       u64le  manifest_offset   Absolute offset of JSON manifest
16      u32le  manifest_size     Size of the JSON manifest (before padding)
20      u32le  reserved          Must be 0
```

## JSON manifest

Immediately before the footer, padded to 4 KiB:

```json
{
  "slices": [
    {
      "target": "x86_64-unknown-linux-musl",
      "uname_machine": "x86_64",
      "uname_sys": "Linux",
      "offset": 65536,
      "size": 2956757,
      "sha256": "a1b2c3..."
    },
    {
      "target": "aarch64-apple-darwin",
      "uname_machine": "arm64",
      "uname_sys": "Darwin",
      "offset": 3022293,
      "size": 2476949,
      "sha256": "d4e5f6..."
    }
  ]
}
```

The manifest is human-readable and allows tools to inspect a universal
binary without executing it. Tools reading the footer can locate the
manifest, parse the slice table, and verify SHA-256 checksums.

## Launcher script

The shell script (first 64 KiB) looks like:

```sh
#!/bin/sh
# daedalus universal binary — auto-generated launcher
_arch=$(uname -m)
_os=$(uname -s 2>/dev/null || echo Linux)
_self=$0
_off=_sz=

case "$_arch $_os" in
  "x86_64 Linux") _off=65536; _sz=2956757 ;;
esac
case "$_arch $_os" in
  "aarch64 Linux") _off=3022293; _sz=2476949 ;;
esac
case "$_arch $_os" in
  "riscv64 Linux") _off=5499242; _sz=2362661 ;;
esac
case "$_arch $_os" in
  "x86_64 Darwin") _off=7861903; _sz=1524141 ;;
esac
case "$_arch $_os" in
  "arm64 Darwin") _off=9386044; _sz=1287101 ;;
esac

if [ -z "$_off" ]; then
  echo 'daedalus: unsupported architecture: '"$_arch"' on '"_os" >&2
  exit 1
fi

_tmpf=$(mktemp /tmp/daedalus.XXXXXX)
tail -c +$((_off + 1)) "$_self" 2>/dev/null | head -c $_sz > "$_tmpf" || \
dd if="$_self" of="$_tmpf" bs=1 skip=$_off count=$_sz 2>/dev/null
chmod +x "$_tmpf"
exec "$_tmpf" "$@"
```

### Extraction strategy

The launcher uses `tail -c +N` piped to `head -c M` for fast extraction.
This is much faster than `dd bs=1` (which reads byte-by-byte) and
correctly handles non-block-aligned offsets. The `dd bs=1` fallback is
used if `tail`/`head` are unavailable or fail.

## Supported targets

| uname -m   | uname -s | target triple                | notes          |
|------------|----------|------------------------------|----------------|
| x86_64     | Linux    | x86_64-unknown-linux-musl    | static ELF     |
| aarch64    | Linux    | aarch64-unknown-linux-musl   | static ELF     |
| riscv64    | Linux    | riscv64gc-unknown-linux-musl | static ELF     |
| x86_64     | Darwin   | x86_64-apple-darwin          | Mach-O         |
| arm64      | Darwin   | aarch64-apple-darwin         | Mach-O         |

Linux slices use musl for static linking (no glibc dependency). macOS
slices use the system dynamic linker but the stub itself is
freestanding (minimal libc with `-undefined dynamic_lookup`).

## Building

```bash
# Build a universal binary for all 5 supported targets
daedalus build ./app --universal --isolation 0 -o app.daedalus

# The CLI builds each slice individually via cargo zigbuild/musl/cargo
# cross-compiler, then assembles them with the polyglot launcher.
```

Implementation lives in `daedalus-core/src/universal.rs` (shared by
the CLI builder and any inspection tools).
