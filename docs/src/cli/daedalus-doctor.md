# `daedalus doctor`

Diagnose the daedalus environment and prerequisites.

```bash
daedalus doctor [OPTIONS]
```

Checks:

- Rust toolchain and musl target
- C compiler (`gcc` / `musl-tools`)
- `zstd` availability
- Stub binary presence
- Key directory and trusted keys directory
- Cache directory writability

## Options

| Flag | Description |
|---|---|
| `--fix` | Attempt to auto-install missing prerequisites |
| `--strict` | Exit with error if any check fails |
| `--plain` | Machine-readable tab-separated output |
| `--quiet` | Suppress non-error output |
| `--no-input` | Disable interactive prompts (required for `--fix` in CI) |

## Examples

```bash
# Run diagnostics
daedalus doctor

# Auto-fix missing prerequisites
daedalus doctor --fix

# Non-interactive fix (CI/scripts)
daedalus doctor --fix --force
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | All checks passed |
| `1` | One or more checks failed (or `--strict` mode) |
