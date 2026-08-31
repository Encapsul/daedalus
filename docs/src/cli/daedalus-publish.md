# `daedalus publish`

Publish a `.daedalus` binary to a registry.

```bash
daedalus publish [OPTIONS] <FILE>
```

Convenience wrapper around `registry push` that extracts and pushes all layers
from a `.daedalus` in one command.

## Options

| Flag | Description |
|---|---|
| `--registry <URL>` | Registry URL |
| `--token <TOKEN>` | Bearer token for authentication |
| `--local <DIR>` | Use a local directory as the registry cache |
| `--verbose` | Show detailed operations |
| `--json` | Emit JSON output |
| `--no-input` | Disable interactive prompts |

## Examples

```bash
# Publish to a remote registry
daedalus publish my-app.daedalus \
  --registry https://registry.example.com \
  --token $DAEDALUS_TOKEN
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Published successfully |
| `1` | Network error or push failure |
