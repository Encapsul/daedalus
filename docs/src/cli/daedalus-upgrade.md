# `daedalus upgrade`

Upgrade daedalus to the latest GitHub release.

```bash
daedalus upgrade [OPTIONS]
```

Fetches the latest release from GitHub, downloads the platform tarball, verifies
the SHA-256 checksum, and installs binaries to the current PATH.

## Options

| Flag | Description |
|---|---|
| `-f, --force` | Skip confirmation prompt |
| `--no-sudo` | Fail if binary is not writable (do not use sudo) |
| `--dry-run` | Show what would be done without doing it |
| `--quiet` | Suppress non-error output |
| `--no-input` | Disable interactive prompts |
| `--verbose` | Show detailed download and install steps |

## How it works

1. Queries the GitHub Releases API for the latest tag.
2. Downloads `daedalus_<version>_<os>_<arch>.tar.gz`.
3. Verifies the embedded checksum.
4. Installs `daedalus`, `daedalus-stub`, and `daedalus-crypto` to a directory
   in `$PATH` (default: `~/.local/bin`).

## Examples

```bash
# Upgrade interactively
daedalus upgrade

# Non-interactive upgrade (CI/scripts)
daedalus upgrade --force --no-input

# Dry run
daedalus upgrade --dry-run
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Upgraded successfully |
| `1` | Network error, checksum mismatch, or install failure |
