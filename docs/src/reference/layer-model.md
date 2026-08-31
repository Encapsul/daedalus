# Layer model

Every `.daedalus` binary is composed of one or more **layers**. Layers are
content-addressed, compressed blobs that can be stored, transferred, and reused
independently.

## Layer kinds

| Kind | Description |
|---|---|
| `runtime` | The interpreter, standard library, and system libraries |
| `app` | The application source code and assets |
| `config` | Environment variables, hooks, and configuration |

## Structure

A layer is a compressed tar or SquashFS archive. Inside each layer:

```
/app/
  <app files...>
```

The launcher extracts each layer into the rootfs in order: runtime first, then
app, then config.

## Addressing

Layers are addressed by their `SHA-256` hash. The metadata block lists the
layers and their hashes, sizes, and compression formats.

## Registry

Layers can be pushed to and pulled from a daedalus registry. The registry
stores:

- `<hash>.layer` — the raw compressed bytes
- `<hash>.json` — metadata (kind, size, compression)

See [Registry HTTP API](./registry-api.md) for the wire format.

## SISR and layers

When SISR is enabled, the payload is chunked into content-addressed pieces.
Unchanged chunks are reused across updates, reducing download size.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Layer operation succeeded |
| `1` | Hash mismatch, corrupt layer, or write failure |
