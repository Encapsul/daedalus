# `daedalus verify`

Verify the Ed25519 signature and integrity of a `.daedalus` binary.

```bash
daedalus verify [OPTIONS] <FILE>
```

Verification steps:

1. Reads the footer and metadata.
2. Verifies the `SHA-256` integrity hash.
3. If a signature block is present, verifies the Ed25519 signature against
   every key in the trusted keys directory.
4. Reports which key signed the binary.

## Options

| Flag | Description |
|---|---|
| `--quiet` | Suppress non-error output |
| `--no-input` | Disable interactive prompts |
| `--json` | Emit a JSON result on stdout |

## Trusted keys directory

Default location: `~/.daedalus/trusted-keys/`.

All `.pub` files in this directory are tried against the signature. Verification
succeeds if at least one trusted key matches.

```bash
# Add a trusted key
cp /path/to/key.pub ~/.daedalus/trusted-keys/

# Verify
daedalus verify my-app.daedalus
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Verified successfully |
| `1` | Integrity failure, missing signature, or no trusted key matched |
