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

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Request succeeded |
| `1` | Network error, auth failure, or layer not found |
