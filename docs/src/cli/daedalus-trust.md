# `daedalus trust`

Add a public key to the trusted keys directory for signature verification,
or list what the recipient machine already trusts.

```bash
daedalus trust [OPTIONS] <PUBKEY_FILE>
daedalus trust --list [OPTIONS]
```

Copies the public key file into the trusted keys directory
(default `~/.daedalus/trusted-keys/`). A recipient machine starts with an
empty store - `daedalus verify` fails until the publisher `.pub` is trusted.

## Options

| Flag | Description |
|---|---|
| `--list` | List fingerprints in the trusted keys directory |
| `--trusted-dir <PATH>` | Trusted keys directory (default `~/.daedalus/trusted-keys/`) |
| `--quiet` | Suppress non-error output |
| `--dry-run` | Show what would be done without doing it |
| `--json` | Emit a JSON result on stdout |

## Examples

```bash
# Trust a public key
daedalus trust ~/.daedalus/keys/ab12cd34.pub

# See what this machine trusts (empty on a fresh recipient)
daedalus trust --list

# Trust with a custom trusted directory
daedalus trust --trusted-dir /etc/daedalus/trusted-keys key.pub
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Key trusted successfully, or `--list` completed |
| `1` | Copy failure, invalid key file, or missing pubkey without `--list` |
