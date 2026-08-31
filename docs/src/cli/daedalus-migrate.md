# `daedalus migrate`

Upgrade a legacy `.daedalus` binary to the SISR-enabled format.

```bash
daedalus migrate [OPTIONS] <INPUT> <OUTPUT>
```

Reads a legacy v1 `.daedalus` (built without `--enable-sisr`) and writes a new
v2 binary with SISR support enabled. The payload is preserved byte-for-byte.

## Options

| Flag | Description |
|---|---|
| `--chunk-size <BYTES>` | SISR chunk target size (default `65536`) |
| `-k, --key <PATH>` | Sign the SISR manifest with a 32-byte Ed25519 key |
| `-f, --force` | Overwrite the output without confirmation |
| `-q, --quiet` | Suppress non-error output |
| `--json` | Emit a JSON result on stdout |
| `--no-input` | Disable interactive prompts |

## Examples

```bash
# Migrate a legacy binary
daedalus migrate old-app.daedalus new-app.daedalus

# Migrate and sign the SISR manifest
daedalus migrate old-app.daedalus new-app.daedalus \
  --key ~/.daedalus/keys/<fingerprint>.key
```

## When to use

- You have a `.daedalus` built before SISR was available.
- You want to add incremental self-update capability without rebuilding the app.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Migrated successfully |
| `1` | Input is not a daedalus binary, already SISR-enabled, or write failure |
