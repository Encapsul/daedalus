# `daedalus selftest`

Run a sandboxed self-test on a `.daedalus` binary.

```bash
daedalus selftest [OPTIONS] <FILE>
```

Executes the binary in a temporary sandboxed environment and reports whether it
starts, responds to health probes (if configured), and exits cleanly.

## Options

| Flag | Description |
|---|---|
| `--verbose` | Show detailed test output |
| `--no-input` | Disable interactive prompts |

## What it tests

- Binary integrity verification
- Extraction to a temporary cache directory
- Entrypoint resolution
- Health endpoint response (if `--health-port` was set at build time)
- Clean shutdown

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Self-test passed |
| `1` | Binary failed to run, health check failed, or extraction error |
