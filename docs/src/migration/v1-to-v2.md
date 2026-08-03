# Migrating from v1 to v2 (SISR)

x.bin v1 packages an app as `[stub][payload][metadata][footer]`. The v2
release introduces **SISR** (Self-Incremental Sovereign Reconstruction):
the same layout with a delta manifest and a fixed access block injected
before the footer, plus **content-addressed delta self-updates**.

This guide explains why your v1 binaries keep working untouched, and the two
ways to migrate them.

## Backward compatibility guarantee

**A v1 `.xbin` is read, extracted, and executed by the v2 runtime exactly as
before — no modification, no warning.** SISR is additive: the launcher gates
on a single flag in the footer, never on the file predating it.

```
                        [ .xbin file to load ]
                                   │
                       Contains SISR header?
                            /            \
                          YES              NO
                           │                │
                  [ v2 (SISR) ]      [ v1 (legacy) ]
                           │                │
            delta auto-update     standard extraction
            supported            only (no update)
```

The v2 runtime decides at load time:

- **`FLAG_SISR` set** → the file embeds a delta manifest; `--xbin-update`
  and the `XBIN_SISR_MANIFEST` path are available.
- **`FLAG_SISR` clear** → classic extraction. Cache keying, integrity
  verification (SHA-256 over `payload ‖ metadata`), and signature checks are
  identical to v1.

Deployment scripts written against v1 remain 100 % valid against v2.

## What an upgrade actually changes

`xbin upgrade-binary <input_v1.xbin> <output_v2.xbin>` performs an **in-place
format promotion**:

```
before: [stub][payload][metadata][footer]
after:  [stub][payload][metadata][manifest][SisrFooterExt][footer]
```

- The stub, payload, and metadata segments are copied **byte-for-byte**. The
  payload is chunked exactly as stored — never decompressed and recompressed.
- The footer integrity hash `SHA-256(payload ‖ metadata)` and any checksum the
  payload embeds (e.g. SquashFS) are therefore preserved by construction.
- `payload_offset`, `meta_offset`, `payload_csize`, `meta_size` are unchanged,
  so a legacy runtime that reads backwards from EOF keeps decoding the
  upgraded file.
- A signed delta manifest (`<output>.xbin.manifest`) is written next to the
  binary, matching a fresh SISR build.

`upgrade-binary` refuses to touch a file that already has SISR, and refuses
**signed** binaries (their signature block sits exactly where the manifest
must be inserted — rebuild those with `xbin build --enable-sisr`).

## Migration paths

### Option A — full rebuild (recommended for new releases)

```bash
xbin build ./app --enable-sisr \
  --update-url https://updates.example.com/app \
  --key ~/.xbin/keys/update.key
```

Best when you can regenerate the app image: you get the current launcher, a
signed manifest, and an embedded update URL in one step.

### Option B — promote an existing v1 binary

```bash
xbin upgrade-binary ./app-old.xbin ./app-new.xbin \
  --key ~/.xbin/keys/update.key
```

Use this to migrate already-deployed binaries without rebuilding the payload.
The embedded launcher is the one from the original build; to gain the current
launcher as well, rebuild (Option A). The promoted binary can be updated
immediately:

```bash
./app-new.xbin --xbin-update https://updates.example.com/app
```

## Deprecation governance

- **v1 (SISR-less)** remains a supported, first-class input for the runtime
  and for `upgrade-binary`. No deprecation warnings are emitted.
- **v2 (SISR)** is the recommended build output going forward.
- The `.xbin` footer format (magic constants, field offsets) is frozen; new
  capabilities ship as flags and format-version bumps that are always
  backwards-readable, never repurposed fields.

## Verifying a migration

```bash
xbin inspect ./app-new.xbin          # flags show SISR
xbin upgrade-binary ./app-new.xbin ./again.xbin
#   → error: input is already SISR-enabled  (expected)
```

The test suite enforces the invariant end-to-end: a legacy binary built with
the current toolchain runs on the v2 launcher with no `[xbin]` output, and an
upgraded binary applies a real delta through a mock update channel.
