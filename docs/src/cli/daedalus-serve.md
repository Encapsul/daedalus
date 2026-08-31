# `daedalus serve`

Start a local layer registry server.

```bash
daedalus serve [OPTIONS]
```

Runs a local HTTP registry that stores and serves daedalus layers. Useful for
testing, air-gapped environments, and local development.

## Options

| Flag | Description |
|---|---|
| `--port <PORT>` | HTTP listen port (default `8080`) |
| `--dir <DIR>` | Storage directory (default `./registry`) |
| `--verbose` | Show request logs |

## Examples

```bash
# Start a local registry on port 8080
daedalus serve

# Custom port and directory
daedalus serve --port 9090 --dir /var/daedalus/registry
```

## API

| Method | Path | Description |
|---|---|---|
| `GET` | `/layers` | List all layers |
| `GET` | `/layers/<hash>` | Download a layer |
| `PUT` | `/layers/<hash>` | Upload a layer |
| `HEAD` | `/layers/<hash>` | Check if a layer exists |

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Server stopped cleanly |
| `1` | Bind failure or I/O error |
