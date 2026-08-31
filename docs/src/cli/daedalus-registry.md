# `daedalus registry`

Interact with the daedalus layer registry.

```bash
daedalus registry push <file> [OPTIONS]
daedalus registry pull <layer> [OPTIONS]
daedalus registry list [OPTIONS]
```

The registry stores content-addressed layers. Push uploads layers from a
`.daedalus`; pull downloads a specific layer; list shows available layers.

## Options

| Flag | Description |
|---|---|
| `--registry <URL>` | Registry URL (use `--local` for a directory) |
| `--token <TOKEN>` | Bearer token for authentication |
| `--local <DIR>` | Use a local directory as the registry cache |
| `--verbose` | Show detailed HTTP / filesystem operations |
| `--plain` | Machine-readable output |
| `--json` | Emit JSON output |
| `--no-input` | Disable interactive prompts |

## Examples

```bash
# Push all layers from a .daedalus to a local registry
daedalus registry push my-app.daedalus --local /tmp/registry

# Push to a remote registry
daedalus registry push my-app.daedalus \
  --registry https://registry.example.com \
  --token $DAEDALUS_TOKEN

# List layers in a local registry
daedalus registry list --local /tmp/registry

# Pull a specific layer
daedalus registry pull <layer-hash> --local /tmp/registry
```

## Layer format

Layers are addressed by their `SHA-256` hash. The registry stores:

- `<hash>.layer` — the raw compressed layer bytes
- `<hash>.json` — metadata (size, kind, compression)

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Operation succeeded |
| `1` | Network error, layer not found, or write failure |
