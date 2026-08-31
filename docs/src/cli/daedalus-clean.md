# `daedalus clean`

Clean the daedalus build cache.

```bash
daedalus clean [OPTIONS]
```

Removes cached extracted rootfs files and build artifacts.

## Options

| Flag | Description |
|---|---|
| `--all` | Remove all cached data |
| `--gc` | Garbage-collect expired cache entries (TTL-based) |
| `-f, --force` | Skip confirmation |
| `--no-input` | Disable all interactive prompts (for CI/scripts) |

## Examples

```bash
# Interactive clean
daedalus clean --all

# Non-interactive clean (CI/scripts)
daedalus clean --all --force --no-input
```

## Cache location

Default cache directory: `~/.cache/daedalus/`.

Each extracted binary gets a directory named after its `SHA-256` hash:
`~/.cache/daedalus/<hash>/rootfs/`.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Cleaned successfully |
| `1` | Cache directory unreadable or removal failure |
