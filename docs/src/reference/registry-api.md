# Registry HTTP API

The daedalus registry is a simple HTTP key-value store for content-addressed
layers.

## Base URL

```
https://registry.example.com/
```

## Endpoints

### List layers

```http
GET /layers
```

Response:

```json
[
  {
    "hash": "sha256:abc123...",
    "kind": "runtime",
    "size": 12345678,
    "compression": "zstd"
  }
]
```

### Download a layer

```http
GET /layers/<hash>
```

Returns the raw compressed layer bytes with `Content-Type: application/octet-stream`.

### Upload a layer

```http
PUT /layers/<hash>
```

Body: raw compressed layer bytes.

Requires authentication via `Authorization: Bearer <token>`.

### Check existence

```http
HEAD /layers/<hash>
```

Returns `200 OK` if the layer exists, `404 Not Found` otherwise.

## Authentication

Pass the bearer token with every write request:

```bash
curl -H "Authorization: Bearer $DAEDALUS_TOKEN" \
  -X PUT \
  --data-binary @my-layer.layer \
  https://registry.example.com/layers/<hash>
```

## Local registry

For air-gapped or development use, run a local registry:

```bash
daedalus serve --port 8080 --dir /tmp/registry
```

## Runnable artifacts (`daedalus serve`)

Beyond content-addressed layers, `daedalus serve start` stores the **full
runnable `.de` binary** under a `name:tag` alias so apps can be distributed
and executed by name.

Implemented endpoints:

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/list` | newline-separated layer hashes |
| `GET` | `/pull/<hash>` | layer JSON by content hash |
| `POST` | `/push` | store a layer (JSON body) |
| `GET` | `/artifacts` | JSON list of `{name, tag, binary_hash, size}` |
| `GET` | `/artifact/<name>:<tag>` | download the runnable `.de` (octet-stream) |
| `POST` | `/artifact?name=<n>&tag=<t>` | store a runnable `.de` (binary body) |

### CLI flows

```bash
# publish a built binary
daedalus build app --publish http://host:8080          # stores as app:latest
daedalus registry push app.de --name app:v2 --registry http://host:8080

# fetch a runnable binary by name
daedalus registry pull --name app:v2 --registry http://host:8080 -o app.de

# .. or run it straight from the registry (npx-style)
daedalus run http://host:8080/artifact/app:v2          # registry://host/... also works
```

`daedalus run <url>` downloads to `~/.cache/daedalus/downloads` (keyed by the
URL's SHA-256), validates the `.de` footer, marks it executable, and reuses
the cached copy on later runs. `registry://` is an alias for `http://`.

Documentation of a remote hosted registry follows. `/layers` endpoints, PUT,
and HEAD are part of the hosted-registry contract:

### List layers

```http
GET /layers
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Request succeeded |
| `1` | Network error, auth failure, or layer not found |
