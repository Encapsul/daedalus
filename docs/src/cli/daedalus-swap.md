# `daedalus swap`

Atomically replace a layer inside a `.daedalus` binary.

```bash
daedalus swap [OPTIONS] <BINARY> <LAYER> <FILE>
```

Replaces a specific layer in an existing `.daedalus` with new content from a
file. Used by the SISR update engine to apply delta updates.

## Options

| Flag | Description |
|---|---|
| `-f, --force` | Overwrite without confirmation |
| `-q, --quiet` | Suppress non-error output |
| `--json` | Emit a JSON result on stdout |
| `--no-input` | Disable interactive prompts |

## Examples

```bash
# Replace the app layer
daedalus swap my-app.daedalus app ./new-app.tar.zst
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Swapped successfully |
| `1` | Layer not found, hash mismatch, or write failure |
