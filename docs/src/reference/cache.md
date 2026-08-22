# Cache

Extracting the rootfs on every launch would be slow. `daedalus` extracts **once**
and reuses.

## Layout

```
~/.cache/daedalus/
  {sha256-of-payload}/
    rootfs/      ← extracted filesystem, ready to use
    .ready       ← marker: extraction is complete and valid
```

The cache key is the **SHA-256 of the compressed payload**. Two `.ere` with
identical content share the same cache; changing a single byte produces a new
key. (`.lock` for concurrent access and `last_used` for LRU cleanup are
planned — see [Roadmap](../roadmap.md).)

## Atomic extraction (anti-TOCTOU)

The danger: between checking that the cache exists and using it, an attacker
could inject content (Time Of Check To Time Of Use attack). The defense is
to **never** expose an intermediate state:

```
1. extract to  ~/.cache/daedalus/.tmp-{pid}-{nanos}/   ← unique directory
2. write .ready once extraction is complete
3. rename() to ~/.cache/daedalus/{sha256}/              ← atomic on Linux
4. if another instance won the race → discard our tmp
```

`rename()` is **atomic** on the same filesystem: either the final directory
exists and is complete, or it doesn't. No half-written state. This is why the
tmp directory is created in the **same** parent directory as the target.

## Cold start vs warm start

| | First execution | Subsequent executions |
|---|---|---|
| Cache | missing → extraction | present (`.ready`) → reused |
| Message | `cold start: extracting...` | `warm start: cache hit {hash}` |
| Cost | zstd decompression + disk write | near-zero from daedalus side |

> Today, warm "time to first HTTP byte" is dominated by the **embedded
> runtime boot** (Python interpreter startup, imports), not by `daedalus`. The
> launcher's own overhead is on the order of milliseconds. The < 100 ms
> end-to-end goal will require squashfs+mmap (no extraction) in Phase 3.

## Two separate caches

| | Extraction cache | Build cache |
|---|---|---|
| Path | `~/.cache/daedalus/{sha256}/` | `~/.cache/daedalus/build/{hash}.zst` |
| Side | **target** machine (at `run`) | **build** machine (at `build`) |
| Content | extracted rootfs, ready to use | **compressed** reusable layers |
| Role | avoid re-extracting on every launch | avoid recompressing on rebuild |

The build cache is what makes a rebuild go from ~25 s to ~1 s (runtime layer
reused). See [`.ere` Format](./format.md#layers-v2).

## Extraction cache key in v2

In v2, the key is not the payload hash but the **SHA-256 of the concatenation
of all layer hashes**. As long as the layers are identical, the extracted
entry is reused. (Per-layer reuse — extracting the runtime layer once and
overlaying app layers via overlayfs — will come with isolation level 2;
today an app layer change re-extracts everything on the target side, but the
build-time gain is already achieved.)

## Cleanup

```bash
daedalus clean        # clear extracted cache entries, KEEP build cache
daedalus clean --all  # wipe all ~/.cache/daedalus (build cache included)
```
