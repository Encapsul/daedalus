# `daedalus scan`

Scan directories for `.daedalus` files and display their metadata.

```bash
daedalus scan [PATHS]...
```

Recursively finds `.daedalus` files in the given paths (defaulting to the
current directory) and displays their metadata in plain, JSON, or paged format.

## Options

| Flag | Description |
|---|---|
| `--json` | Emit JSON output |
| `--plain` | Machine-readable tab-separated output |
| `-o, --output <PATH>` | Write JSON output to a file |
| `--dry-run` | Show what would be done without doing it |
| `--cache` | Show cache statistics |
| `--no-input` | Disable interactive prompts |

## Examples

```bash
# Scan current directory
daedalus scan

# Scan specific directories
daedalus scan ./dist /opt/apps

# JSON output
daedalus scan --json > inventory.json
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Scanned successfully |
| `1` | Read error or invalid `.daedalus` file encountered |
