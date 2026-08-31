# Remote cache

daedalus supports a remote build cache to share compiled runtime layers across
machines and CI pipelines.

## Configuration

```bash
daedalus build ./my-app -o my-app.daedalus \
  --use-cache \
  --remote-cache-url https://cache.example.com \
  --remote-cache-max-entries 100
```

## Options

| Flag | Description |
|---|---|
| `--use-cache` | Enable build caching |
| `--clear-cache` | Clear the local cache before building |
| `--remote-cache-url <URL>` | Remote cache server URL |
| `--remote-cache-max-entries <N>` | Maximum entries in the remote cache |

## Cache keys

Cache entries are keyed by:

- Runtime version (e.g., Python 3.12.4)
- Target architecture
- Isolation level
- Compression settings

## Remote cache server

The remote cache is a simple HTTP key-value store:

| Method | Path | Description |
|---|---|---|
| `GET` | `/cache/<key>` | Download a cached layer |
| `PUT` | `/cache/<key>` | Upload a cached layer |
| `HEAD` | `/cache/<key>` | Check if a layer exists |

## Examples

```bash
# Build using remote cache
daedalus build ./my-app -o my-app.daedalus \
  --use-cache \
  --remote-cache-url https://cache.example.com

# Clear local cache
daedalus build ./my-app -o my-app.daedalus --clear-cache
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Built successfully |
| `1` | Cache server error or write failure |
