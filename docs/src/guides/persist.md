# Persistent storage

daedalus can preserve a writable data directory across runs using the
`--persist` flag.

## Build

```bash
daedalus build ./my-app -o my-app.daedalus --persist
```

Creates a persistent data directory at:

```
~/.local/share/daedalus/<app-name>/
```

This directory survives binary updates and is shared across all runs of the same
app.

## Use cases

- SQLite databases
- Uploaded files
- Generated assets
- Session data

## Behavior

- The persist directory is bind-mounted into the sandbox at `/app/data/`.
- It is excluded from the payload and SISR chunking.
- On SISR updates, the persist directory is preserved atomically.

## Options

| Flag | Description |
|---|---|
| `--persist` | Enable persistent storage for the app |

## Example

```bash
# Build with persistence
daedalus build ./my-app -o my-app.daedalus --persist

# Run — data persists across updates
./my-app.daedalus
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Ran successfully |
| `1` | Persist directory creation failure |
