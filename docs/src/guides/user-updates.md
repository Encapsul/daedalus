# Updating a deployed `.ere` (SISR)

A `.ere` built with `--enable-sisr` can update itself **in place** from a
signed delta: it downloads only the chunks that changed, verifies every byte,
and atomically swaps in the new binary. No toolchain, no `erebus` CLI, no
extraction on the target machine — the update runs inside the binary itself.

```
DEV MACHINE                        TARGET MACHINE

erebus build ./app -o app.ere \
  --enable-sisr --key …           ./app.ere --erebus-update
  --update-url https://…            │  fetch manifest (HTTPS)
   │                                │  verify signature + Merkle root
   └─ publish to update server ───► │  download changed chunks
        app.ere.manifest          │  SHA-256-verify every chunk
        chunks/<sha256>            │  atomic in-place swap
                                   ▼
                               app.ere is now the new version
```

## 1. Build an updatable binary

```bash
erebus build ./my_app -o my_app.ere \
    --enable-sisr \
    --key $XDG_DATA_HOME/erebus/keys/<fingerprint>.key \
    --update-url https://updates.example.com/my_app
```

Two artifacts are produced:

| Artifact | Purpose |
|---|---|
| `my_app.ere` | Self-extracting binary; self-updates against the channel |
| `my_app.ere.manifest` | Signed `XBMR` manifest, published to the server |

## 2. Publish a release

Publish the manifest and the content-addressed chunks. Each chunk is served at
`<hash>` = the 64-hex SHA-256 of its bytes, so a fetched chunk can be verified
before use:

```
https://updates.example.com/my_app/
  manifest                     ← the .ere.manifest (XBMR, signed)
  chunks/
    <64-hex-sha256>            ← one file per chunk
    <64-hex-sha256>
    …
```

Any HTTP server works (nginx, S3, a static CDN). HTTPS is recommended — the
launcher uses TLS, though the trust anchor is the manifest signature and the
content hashes, not the transport.

The publisher is responsible for keeping `manifest` and the `chunks/` files
in sync for the *newest* release.

## 3. Update on the target

```bash
# Uses the URL embedded at build time
./my_app.ere --erebus-update

# Or point at a specific channel
./my_app.ere --erebus-update https://updates.example.com/my_app

# Or override per-run via the environment
EREBUS_UPDATE_URL=https://updates.example.com/my_app ./my_app.ere --erebus-update
```

Progress and statistics are printed on stderr (stdout is reserved for the app):

```
[erebus] update: fetching manifest from https://updates.example.com/my_app/manifest
[erebus] update: manifest verified (247 chunks)
[erebus]   fetched chunk 1/247 (65536 bytes)
[erebus]   fetched chunk 2/247 (65536 bytes)
…
[erebus] update applied: 203 chunks reused (12.7 MiB), 44 chunks fetched (2.8 MiB), total 247
[erebus] updated binary: /home/alice/app.ere
```

The `reused` number is the bandwidth you saved: those chunks were already
present in the running binary and were copied in place, never downloaded.

## Other runtime flags

| Flag | Effect |
|---|---|
| `--erebus-update [URL]` | Check the channel, verify, apply the delta, print stats, exit |
| `--erebus-version` | Print the erebus stub and app versions, then exit |

Both are reserved by erebus: they are intercepted by the launcher and are never
forwarded to the application.

## Security model

Nothing is trusted from the network:

1. The manifest is verified with Ed25519 against the trusted keys in
   `~/.ere/trusted-keys/` (same keys the binary signature uses) **before**
   a single byte is written.
2. The Merkle root must match the manifest's own chunk table.
3. Every chunk — reused or downloaded — must SHA-256 to its manifest entry;
   wrong length or hash is rejected on the spot.
4. The swap is atomic: any failure (bad signature, missing chunk, power loss)
   leaves the previous binary intact and runnable.
5. The first run of the new version is supervised (mission 8): if it crashes
   during the startup window, the previous version is restored automatically
   and a failing release is quarantined so it is not re-installed.

## Failure behavior

| Situation | Outcome |
|---|---|
| Manifest missing / bad magic / bad URL | update refused, binary untouched |
| Signature fails (key not trusted) | update refused before any write |
| Merkle root mismatch | update refused before any write |
| Chunk wrong length or SHA-256 | engine errors, binary untouched |
| Chunk 404s on the server | engine errors, binary untouched |
| Interruption mid-update | previous binary intact |
| New version crashes at startup | `.bak` restored, previous version runs |
| Release fails `EREBUS_HEALTH_MAX_ATTEMPTS` times | quarantined, installs refused |
| Re-install of a quarantined release | refused before any write |

The running binary is always the last **valid** version — there is no
"half-updated" state, and a broken release cannot wedge a target machine.

## More

- [Incremental Updates (SISR)](./incremental-updates.md) — the full workflow
  for publishers.
- [Runtime Launcher](../architecture/runtime-launcher.md) — what the launcher
  does step by step.
- [`erebus build`](../cli/erebus-build.md) — the builder flags.
