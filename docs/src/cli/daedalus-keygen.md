# `daedalus keygen`

Generate an Ed25519 keypair for signing `.daedalus` binaries.

```bash
daedalus keygen [OPTIONS]
```

Creates two files in the key directory:

- `<fingerprint>.key` — 32-byte private key (mode `0600` on Unix)
- `<fingerprint>.pub` — public key

The fingerprint is the hex-encoded SHA-256 of the public key.

## Options

| Flag | Description |
|---|---|
| `--key-dir <PATH>` | Directory for keys (default `.`) |
| `--quiet` | Suppress non-error output |
| `--force` | Overwrite existing keys |
| `--no-input` | Disable interactive prompts |
| `--json` | Emit a JSON result on stdout |

## Examples

```bash
# Generate keys in the default location
daedalus keygen

# Generate keys in a custom directory
daedalus keygen --key-dir ~/.daedalus/keys

# Overwrite existing keys
daedalus keygen --force
```

## Security

- The private key is zeroized in memory after use.
- On Unix, the key file is created with mode `0600`. A warning is printed if
  the permissions are insecure.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Keypair generated successfully |
| `1` | Keypair already exists (without `--force`) or write failure |
