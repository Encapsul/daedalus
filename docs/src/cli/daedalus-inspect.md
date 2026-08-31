# `daedalus inspect`

Inspect the metadata and structure of a `.daedalus` binary.

```bash
daedalus inspect [OPTIONS] <FILE>
```

Displays the embedded metadata: runtime, entrypoint, layers, integrity hash,
signature status, and optional SBOM.

## Options

| Flag | Description |
|---|---|
| `--json` | Emit JSON output |
| `--plain` | Machine-readable tab-separated output |
| `-o, --output <PATH>` | Write JSON output to a file |
| `--dry-run` | Show what would be done without doing it |
| `-S, --sbom` | Generate SPDX SBOM instead of default metadata |

## Example output

```
name:            hello-web
runtime:         python
entrypoint:      /usr/bin/python3.12 /app/app.py
layers:
  - runtime   11.9MB compressed / 54.0MB raw
  - app        0.0MB compressed /  0.0MB raw
integrity sha256: 4232327e...
signed:          yes (key fingerprint: ab12cd34...)
```

## SBOM

```bash
daedalus inspect my-app.daedalus --sbom > sbom.json
```

Generates an SPDX JSON Software Bill of Materials listing all embedded files
and their hashes.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Inspected successfully |
| `1` | File not found, corrupt, or not a `.daedalus` |
