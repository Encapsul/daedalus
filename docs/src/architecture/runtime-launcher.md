# Runtime Launcher: SISR self-update

> Status: **implemented** (`xbin-core` `sisr::engine` + `sisr::swap`, wired into
> `stub/src/main.rs`). Describes how the launcher rebuilds the running binary
> from a signed delta manifest before extracting and executing it. Since
> mission 8 the launcher also supervises the first run of a freshly updated
> version and rolls back automatically if it crashes — see
> [Rollback & Resilience](../concepts/rollback-and-resilience.md).

The launcher flow in `stub/src/main.rs` is deliberately linear: locate self
via `/proc/self/exe`, read the footer and metadata, apply a SISR update if one
is requested, then extract the payload and `exec`. The SISR step sits between
metadata parsing and the cache/extract phase, so the **same run** executes the
new version.

Two trigger paths exist:

- **Local staging** (mission 6): `$XBIN_SISR_MANIFEST` points at a signed
  manifest whose chunks are staged in `<manifest-dir>/chunks/`. The launcher
  stays network-free; this path is unchanged.
- **Remote update** (mission 7): `./app.xbin --xbin-update [URL]` — the
  launcher intercepts the flag before the app sees it, downloads the manifest
  and the changed chunks over HTTPS, applies the delta, prints reuse/fetch
  statistics, and exits. The URL resolution order is
  `--xbin-update <URL>` argument > `$XBIN_UPDATE_URL` > the `update_url`
  embedded at build time (`xbin build --update-url`). `--xbin-version` prints
  version info and exits.

```
./app.xbin  (XBIN_SISR_MANIFEST=/updates/app.xbin.manifest   — local staging)
            (./app.xbin --xbin-update                        — remote update)
   │
   1. open /proc/self/exe → footer → metadata
   2. update requested?
        no  → skip to 6
        yes ↓
   3a. local: read + parse remote manifest (XBMR) from $XBIN_SISR_MANIFEST
   3b. remote: GET {base}/manifest (HTTPS) and parse it
   4. verify Ed25519 signature against trusted keys (~/.xbin/trusted-keys/)
      verify Merkle root against the chunk table
   5. SisrEngine::apply_update(/proc/self/exe, manifest, fetcher)
        - reuse unchanged chunks from the current binary
        - fetch the rest — local: <manifest-dir>/chunks/<hex-hash>
                          remote: GET {base}/chunks/<hex-hash> (HTTPS)
        - SHA-256-verify every chunk before writing
        - write to .tmp, fsync, rename → atomic swap
       failure at any point ⇒ binary untouched, launcher exits with error
    6. re-open the *canonical real path* returned by the engine (not
       /proc/self/exe, which can still resolve to the pinned pre-update inode)
       and re-read footer + metadata — now the new version
    7. health gate (mission 8): snapshot `./app.xbin.bak` taken before the
       swap; the new version is supervised for its startup window
         - healthy  ⇒ confirmed, `.bak` discarded
         - crashing ⇒ recorded, `.bak` restored atomically, previous version runs
         - quarantined target ⇒ refused before the swap
    8. cache check → extract → exec as usual
```

The `--xbin-update` / `--xbin-version` paths are **terminal**: after the
update the launcher prints statistics on stderr and exits without exec'ing the
app, so those flags never reach the host application.

## Trigger and chunk location

- `$XBIN_SISR_MANIFEST` — path to a signed [remote manifest]
  (`RemoteManifest::from_bytes`); when unset the launcher is stock.
  Chunk files are read from the `chunks/` directory **next to the manifest**
  (`<manifest-dir>/chunks/<64-hex-sha256>`), served by
  [`DirectoryChunkFetcher`].
- `--xbin-update [URL]` — fetches `<URL>/manifest` and `<URL>/chunks/<hex>`
  over HTTPS via [`HttpChunkFetcher`], resolving the base URL from the
  positional argument, then `$XBIN_UPDATE_URL`, then the embedded
  `update_url`. The transport is never a trust anchor: the manifest is
  signature-verified and every chunk is SHA-256-verified by the engine before
  it is written.
- When every chunk is already present in the running binary, no chunk file is
  touched and the server may not even be asked for chunks.

## Security ordering (non-negotiable)

The manifest is **authenticated before a single byte is written**:

1. `RemoteManifest::verify_any(trusted_keys)` — the Ed25519 signature over
   `merkle_root ‖ manifest_bytes` must verify against at least one key in the
   trusted-keys directory (same directory the embedded binary signature uses,
   see `load_trusted_keys`).
2. `RemoteManifest::verify_merkle()` — the stated Merkle root must match the
   chunk table, so a signer error cannot smuggle a mismatched root.
3. Per-chunk, inside the engine: every byte written — whether **reused** from
   the current binary or **fetched** — must SHA-256 to its `ChunkEntry.hash`.
   A fetched chunk of the wrong length or hash is rejected on the spot.

Only then does the engine assemble and atomically swap.

## Atomicity guarantees

`sisr::swap::AtomicWriter` writes to `.<tag>.tmp-<pid>` in the same directory,
`fsync`s, then `rename(2)`s over the destination:

- an error or an interruption **before** commit leaves the original binary
  byte-for-byte intact (the temp file is removed on drop);
- the engine opens and hashes from the canonical path, and the swap is a
  single atomic rename — there is no "half-updated" state;
- the source file's mode is copied onto the temp file before the rename, so a
  replaced binary keeps its executable bit (`File::create` alone would yield
  `0o644`);
- after the swap the launcher re-opens the **canonical real path** returned by
  the engine — `/proc/self/exe` can keep resolving to the pinned pre-update
  inode — and continues with the new footer and payload.

## Post-update health gate

Atomicity alone does not protect against a *valid but broken* update. Before
the swap the launcher snapshots the running binary to `./app.xbin.bak`
(same filesystem → atomic restore); after the swap it supervises the new
version for its startup window (`XBIN_HEALTH_TIMEOUT_MS`, default 10 s):

- the new version exits 0, or is still running when the window closes →
  `health_store` marks it healthy and the snapshot is discarded;
- the new version crashes or exits non-zero → a failure is recorded; once
  `attempts >= XBIN_HEALTH_MAX_ATTEMPTS` (default 3) the version is
  **quarantined** and the snapshot is restored, after which the previous
  version runs;
- a quarantined target is refused at the top of the update path — **before**
  any snapshot or engine I/O — so a broken release cannot be re-installed in
  a loop.

Health records are JSON files in `~/.cache/xbin/health/`, keyed by the target
version's content hash. See
[Rollback & Resilience](../concepts/rollback-and-resilience.md) for the full
state machine.

## What is copied, what is rebuilt

| Piece             | Handling                                                     |
|-------------------|--------------------------------------------------------------|
| stub (0..payload_offset) | copied verbatim from the current binary                |
| payload chunks    | reused when the current binary already has hash-verified bytes, else fetched + hash-verified |
| metadata          | copied verbatim from the current binary (deltas never change metadata) |
| `DeltaManifest` + `SisrFooterExt` | rebuilt from the fetched manifest and chunk table |
| footer            | rebuilt: `flags = (old & ~FLAG_SIGNED) \| FLAG_SISR`, payload SHA-256 = `SHA-256(payload ‖ meta)`, signature offset zeroed |

A pre-SISR (legacy) binary is handled too: there is no embedded chunk index, so
the engine falls back to fetching every chunk — correct, just not incremental.

## Failure table

| Situation | Outcome |
|---|---|
| `XBIN_SISR_MANIFEST` unreadable / bad magic | launcher exits, binary untouched |
| Manifest URL unreachable / non-2xx | update refused, binary untouched |
| No update URL resolvable (`--xbin-update` alone) | update refused with guidance |
| Signature fails (no trusted key verifies) | update refused before any write |
| Merkle root mismatch | update refused before any write |
| Fetched chunk wrong length or SHA-256 | engine errors, binary untouched |
| Chunk missing from `chunks/` (local) or 404s (remote) | engine errors, binary untouched |
| `rename` fails (read-only dir) | engine errors, binary untouched |
| Power loss / `SIGKILL` mid-write | `.tmp` may remain, binary untouched |
| Update applies, new version crashes at startup | `.bak` restored atomically, previous version runs, failure recorded |
| Version fails `XBIN_HEALTH_MAX_ATTEMPTS` times | quarantined; previous version runs |
| Re-install of a quarantined version | refused before any snapshot or write |

## Related

- [Builder Pipeline](./builder-pipeline.md) — where the manifest and chunks are
  produced and signed.
- [SISR: Self-Incremental Sovereign Reconstruction](./sisr-spec.md) — trust
  model and invariants.
- [`.xbin` Format v2 — SISR extension](../spec/xbin-format-v2.md) — byte layout
  of the remote manifest and the footer extension.
- [Incremental Updates (SISR)](../guides/incremental-updates.md) — the
  end-to-end workflow the launcher completes.

[remote manifest]: ../spec/xbin-format-v2.md
[`DirectoryChunkFetcher`]: ../architecture/internal-crates.md
[`HttpChunkFetcher`]: ../architecture/internal-crates.md
