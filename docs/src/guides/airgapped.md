# Air-gap release train

Ship **one signed ELF + `.pub` + SISR media** - not a registry pull, not another
runtime. The private key never rides on the USB.

```
PUBLISHER (online or build host)          RECIPIENT (air-gapped)

daedalus keygen --key-dir ./keys
daedalus build ./app -o app-v1.de \
  --enable-sisr --key ./keys/<id>.key
daedalus build ./app -o app-v2.de \       daedalus trust publisher.pub
  --enable-sisr --key ./keys/<id>.key     # empty / foreign trust → refuse
daedalus media stage app-v2.de \          DAEDALUS_SISR_MANIFEST=…/manifest \
  -o /media/usb/update \                    ./app-v1.de
  --against app-v1.de \
  --pubkey ./keys/<id>.pub
        │
        └── USB / courier ───────────────►  manifest + chunks/ + .pub
```

Synthetic SISR fixtures in CI (fixed byte ratios) are **not** this chain. The
train below uses a real content-defined delta against a prior artifact.

## 1. Publisher: keys and builds

```bash
daedalus keygen --key-dir ./keys
# Keep *.key offline. Only *.pub leaves the build host.

daedalus build ./my-app -o app-v1.de \
  --enable-sisr \
  --key ./keys/<fingerprint>.key

# Change the app, then rebuild v2 with the same key.
daedalus build ./my-app -o app-v2.de \
  --enable-sisr \
  --key ./keys/<fingerprint>.key
```

With `--enable-sisr`, `--key` signs the **SISR manifest** (not a trailing
binary signature block). Cold start still verifies that signature against
trusted keys.

## 2. Stage removable media

```bash
daedalus media stage app-v2.de \
  -o /media/usb/update \
  --against app-v1.de \
  --pubkey ./keys/<fingerprint>.pub
```

Layout written:

```
/media/usb/update/
  manifest                 # signed XBMR (same bytes as app-v2.daedalus.manifest)
  chunks/<64-hex-sha256>   # only chunks absent from --against (delta)
  <fingerprint>.pub        # optional; never a .key
```

`--against` is optional. Omit it to seed a full channel (every chunk).
`--pubkey` refuses non-`.pub` paths so a private key cannot be copied by
mistake.

Before the courier leaves (or on the recipient machine), check the tree
offline:

```bash
# Delta media: pass the same --against used at stage time
daedalus media verify /media/usb/update \
  --against app-v1.de \
  --pubkey ./keys/<fingerprint>.pub
```

Empty trust / foreign key / missing chunk / bad hash → exit 1. A `.pub` in
the media root is accepted for this check (still never a `.key`).

## 3. Recipient: trust, then apply

```bash
# Fails closed until the publisher pub is trusted
daedalus trust /media/usb/update/<fingerprint>.pub

# SISR-signed .de (no trailing v3 block): verify still authenticates the manifest
daedalus verify app-v1.de

# Local apply (no network): point the installed v1 binary at the media root
DAEDALUS_SISR_MANIFEST=/media/usb/update/manifest ./app-v1.de
```

The launcher reads `chunks/` next to the manifest, verifies Ed25519 + Merkle
**before any write**, reuses chunks already in the binary, fetches the rest
from media, and atomically swaps. Empty trust or a foreign key refuses the
update.

### Optional: localhost HTTP from the USB tree

If you prefer `--daedalus-update` over the env var, serve the **same** layout
from the media root (the stub expects `{base}/manifest` and `{base}/chunks/<hex>`;
`file://` is not a transport):

```bash
python3 -m http.server --directory /media/usb/update 8765
./app-v1.de --daedalus-update http://127.0.0.1:8765
```

Trust remains the Ed25519 manifest signature, not the transport.

## 4. First delivery (no prior binary)

Hand the recipient `app-v1.de` + `.pub` on the same courier package:

```bash
daedalus trust publisher.pub
daedalus verify app-v1.de   # fails if trust store is empty
./app-v1.de
```

See `examples/field-kit` modes `handoff` and `update` for the checklist printed
from a packaged `.de`.

## Honesty checklist

| Claim | Reality |
|-------|---------|
| Private key on USB | **Never.** Only `.pub` + media. |
| CI synthetic SISR fixtures | Size-ratio tests ≠ production chunk chain. |
| Registry / `--local` layers | Optional CAS tooling; **not** the air-gap train. |
| `daedalus upgrade-binary --local` | Migrates legacy → SISR; does **not** apply USB deltas. |

## Exit codes

| Code | Meaning |
|------|---------|
| `0` | Media staged / media verified / update applied / binary verify ok |
| `1` | Trust failure, bad signature, missing chunk, or staging error |
