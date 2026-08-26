# Incremental Updates (SISR)

> **Advanced, opt-in.** This guide covers the SISR extension: making a `.daedalus`
> able to update itself from signed deltas. If you don't need self-updates,
> ignore this page — a plain `daedalus build` already gives you a static,
> self-contained binary.
>
> Status: **implemented** — `daedalus build --enable-sisr` produces an updatable
> binary and its signed manifest, and `./app.daedalus --daedalus-update` applies the
> delta on the target (see [User-updates](./user-updates.md) for the
> end-to-end workflow).

## When to use SISR

Use an updatable binary when you ship a fix or a feature to **many targets**
and rebuilding + redistributing the whole file each time is the bottleneck:

- fleets of servers behind slow links,
- security patches that must land fast,
- many identical deployments that can share deltas.

If you deploy to a handful of machines and can rebuild, SISR is overkill. The
default static `.daedalus` remains the right choice.

## Overview of the workflow

```
DEV MACHINE                          TARGET MACHINE
                                     (no toolchain, no daedalus CLI)

build + sign v1.0  ──────────────►   ./app.daedalus --daedalus-update
build + sign v1.1                     │  fetch manifest (HTTPS)
   │                                  │  verify signature + anti-rollback
   │                                  │  download changed chunks
   │  └─ publish manifest + chunks ─► │  verify hashes + Merkle root
   │        (remote / CDN / S3)       │  rebuild + atomic swap
   │                                  ▼
   │                              running v1.1
```

Three things are produced per release: the `.daedalus` itself (for fresh
installs), the **manifest**, and the **chunks** it references. Both the
manifest and every chunk are verifiable without any tool — the verification
runs inside the binary.

## 1. Build an updatable binary

```bash
# Static container — no SISR
daedalus build ./my_app -o my_app.daedalus

# Updatable binary — enables SISR and embeds the update channel
daedalus build ./my_app -o my_app.daedalus \
    --enable-sisr \
    --key $XDG_DATA_HOME/daedalus/keys/<fingerprint>.key \
    --update-url https://updates.example.com/my_app
```

The two build the same payload. `--enable-sisr` additionally content-chunks
the payload, embeds the initial trust policy (your signing key) and the update
channel into the binary, and writes `<output>.manifest` — the signed delta
manifest that gets published.

## 2. Sign the release

Signing a release uses the same Ed25519 keys as daedalus's existing signing
support, but the **object** signed differs when SISR is on:

```bash
daedalus keygen --key-dir $XDG_DATA_HOME/daedalus/keys
daedalus build ./my_app --enable-sisr --key $XDG_DATA_HOME/daedalus/keys/<fingerprint>.key --update-url https://…
```

With `--enable-sisr` the `--key` argument signs the **manifest**, not the
binary (the SISR section would be truncated by the binary signature block).
Without `--enable-sisr`, `daedalus sign` still signs the binary as before. The
trust chain is fixed at build time: a `.daedalus` only applies updates signed by
the same key, or by a delegated key recorded in its header.

## 3. Publish the update

For each release, publish:

```
updates.example.com/
  my-app/
    manifest              ← signed XBMR manifest (the .daedalus.manifest)
    chunks/<sha256>        ← content-addressed encoded chunks
```

The chunk names are **content-addressed** (see the
[delta manifest spec](../spec/delta-manifest-format.md)), so a chunk fetched
by name can be verified against its hash before use. `manifest` is the file
`daedalus build` writes next to the binary (`<output>.manifest`). Publish it
under the base URL you passed to `--update-url`, with the `chunks/` directory
beside it.

## 4. Update on the target

```bash
# Check for and apply an update (URL from --update-url, $DAEDALUS_UPDATE_URL,
# or an explicit argument — in that order)
./my_app.daedalus --daedalus-update

# Point at a different channel explicitly
./my_app.daedalus --daedalus-update https://updates.example.com/my_app

# Inspect versions without updating
./my_app.daedalus --daedalus-version
```

What happens under the hood — and what cannot be skipped:

1. the binary fetches its manifest from the remote channel (HTTPS),
2. it verifies the Ed25519 signature **before** doing anything else,
3. it verifies `base_sha256` matches the current binary (right version?),
4. it verifies the Merkle root of the fetched chunk table,
5. it downloads only the chunks it does not already have and verifies each
   (SHA-256 + length),
6. it rebuilds the binary and commits atomically — an interruption leaves the
   previous binary intact.

## 5. Verification of a standalone `.daedalus`

A `.daedalus` produced by SISR is a **standard, valid `.daedalus`**. Existing
verification works unchanged:

```bash
daedalus verify my_app.daedalus --trusted-dir $XDG_DATA_HOME/daedalus/trusted-keys
```

There is no separate "SISR format" to verify.

## Failure behavior (what the user sees)

| Situation | Outcome |
|---|---|
| Manifest not signed / wrong key | update refused, binary untouched |
| Wrong `base_sha256` | update refused (not applicable to this version) |
| Tampered chunk in cache | chunk rejected by content hash |
| Merkle root mismatch | candidate discarded before commit |
| Power loss / `SIGKILL` mid-update | previous binary still intact and running |

In every case the running binary is the last valid version. There is no
"half-updated" state.

## Trade-offs

- **Payload grows** — the SISR engine is embedded in the binary (a few
  hundred KB, statically linked).
- **A trust anchor is fixed at build time** — to rotate the signing key you
  must rebuild and redistribute once.
- **Updates need a reachable manifest** — self-update requires network access
  to the update channel.

None of this applies to a classic `.daedalus`; it stays a zero-dependency static
container.

## References

- [SISR overview](../concepts/sisr-overview.md) — classic vs SISR at a glance.
- [Delta manifest format](../spec/delta-manifest-format.md) — what the engine
  fetches and verifies.
- [SISR conceptual specification](../architecture/sisr-spec.md) — the
  invariants and trust model.
