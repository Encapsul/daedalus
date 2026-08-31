# `daedalus trust`

Add a public key to the trusted keys directory for signature verification.

```bash
daedalus trust [OPTIONS] <PUBKEY_FILE>
```

Copies the public key file into the trusted keys directory
(default `~/.daedalus/trusted-keys/`).

## Options

| Flag | Description |
|---|---|
| `--trusted-dir <PATH>` | Trusted keys directory (default `~/.daedalus/trusted-keys/`) |
| `--quiet` | Suppress non-error output |
| `--dry-run` | Show what would be done without doing it |
| `--json` | Emit a JSON result on stdout |

## Examples

```bash
# Trust a public key
daedalus trust ~/.daedalus/keys/ab12cd34.pub

# Trust with a custom trusted directory
daedalus trust --trusted-dir /etc/daedalus/trusted-keys key.pub
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Key trusted successfully |
| `1` | Copy failure or invalid key file |
