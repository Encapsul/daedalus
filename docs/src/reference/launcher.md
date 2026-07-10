# The Launcher (stub)

The launcher ("stub") is the small program embedded at the head of every
`.xbin`. It's the ELF the kernel runs when the user executes
`./my_app.xbin`.

- **Code**: `stub/src/main.rs` + `stub/src/format.rs`
- **Language**: Rust, statically compiled for `x86_64-unknown-linux-musl` →
  zero dynamic dependencies, runs everywhere.
- **Size**: ~600 KB (target < 200 KB after dependency optimization).

## Why Rust + musl?

**Rust**: the launcher executes before any verification, manipulates binary
files and makes system calls. A memory error here = vulnerability. Rust
eliminates use-after-free, buffer overflows and null dereferences by
construction, with no runtime or GC.

**musl**: `glibc` is dynamically linked and its versions vary between distros
(`GLIBC_2.35 not found`...). A static musl binary has **zero** dynamic
dependencies — it runs on any Linux kernel ≥ 3.8. This is exactly what `xbin`
must guarantee for its own launcher.

```bash
ldd stub/target/x86_64-unknown-linux-musl/release/xbin-stub
# → statically linked
```

## Execution flow

```
./my_app.xbin
   │
   1. open("/proc/self/exe")          ← reliable self-location
   2. read footer (last 84 bytes), validate magic
   3. read JSON metadata
   4. read payload, verify SHA-256         ← integrity
   5. cache hit? → ~/.cache/xbin/{sha256}/.ready
        yes → reuse
        no  → extract (zstd → tar) to tmp, atomic rename()
   6. build argv + env (inject LD_LIBRARY_PATH)
   7. execve(entrypoint)              ← replaces the process
```

## Why `/proc/self/exe` and not `argv[0]`?

`argv[0]` is caller-controlled: a malicious process could launch the launcher
with a fake `argv[0]`. `/proc/self/exe` is provided by the kernel and
**always** points to the real running executable. We always read the correct
file.

## Decompression: why `ruzstd` and not the `zstd` crate?

The `zstd` crate binds the C library `libzstd`, requiring a C compiler for
the musl target. `ruzstd` is a **100% Rust** zstd decompressor: no C
toolchain, trivial static musl build. The launcher only **decompresses** —
that's exactly `ruzstd`'s scope. **Compression** (more CPU-intensive) stays
on the builder side via the `zstd` CLI.

## The `${ROOTFS}` token in the environment

The builder doesn't know in advance where the cache will be materialized
(`~/.cache/xbin/{sha256}/rootfs`). To still declare paths (e.g. `PYTHONPATH`),
it writes the `${ROOTFS}` token in the manifest's environment variables, and
the launcher replaces it with the real path at `exec` time:

```
manifest :  PYTHONPATH = ${ROOTFS}/app/site-packages
execution:  PYTHONPATH = /home/user/.cache/xbin/f342.../rootfs/app/site-packages
```

`LD_LIBRARY_PATH` doesn't need this token: the launcher computes it directly
from the `lib*` directories present in the rootfs.

## CWD handling after pivot_root

When isolation level 2 is active, after `pivot_root + umount2`, the process's
current working directory still points to the old root — which was just
detached. The launcher calls `set_current_dir("/")` just before `execve()`
so the app process starts in the new root, ensuring correct resolution of
relative paths and symlinks.

## Concurrent access: `flock()`

If two instances of the same `.xbin` start simultaneously on a cold cache,
an exclusive `flock()` (on `~/.cache/xbin/{hash}.lock`) guarantees only one
performs the extraction; the other waits and then finds the cache ready.
Extraction is already atomic via `rename()` — `flock` simply avoids duplicated
work.

## What the launcher does at runtime (annotated)

Excerpt from `main.rs` — the sequence is deliberately linear and readable:

```rust
// 1. Locate ourselves reliably (not argv[0], which is caller-controlled).
let mut exe = File::open("/proc/self/exe")?;
let footer = Footer::read_from(&mut exe)?;

// 2-3. Metadata then payload, with integrity verification.
let meta: Metadata = serde_json::from_slice(&meta_bytes)?;
verify_sha256(&payload, &footer.payload_sha256)?;

// 4. Cache: extract once, atomically.
if !ready_marker.exists() {
    extract_atomic(&payload, &cache_root, &rootfs)?;
}

// 5. Replace the current process with the app.
exec_app(&meta, &rootfs)
```
