# `daedalus sign`

Sign a `.daedalus` binary with an Ed25519 key.

```bash
daedalus sign [OPTIONS] <FILE>
```

## How it works

1. Reads the existing `.daedalus` and locates the signature block after the
   metadata.
2. Computes `SHA-256(payload || metadata)`.
3. Signs the hash with the provided Ed25519 key.
4. Writes a new file with the signature block inserted.

The signature covers the payload and metadata only — not the launcher stub.

## Options

| Flag | Description |
|---|---|
| `-k, --key <PATH>` | Path to the 32-byte Ed25519 private key |
| `-f, --force` | Overwrite an existing signature |
| `--quiet` | Suppress non-error output |
| `--no-input` | Disable interactive prompts |
| `--json` | Emit a JSON result on stdout |

## Key generation

```bash
daedalus keygen --key-dir ~/.daedalus/keys
```

Creates `<fingerprint>.key` (private) and `<fingerprint>.pub` (public) in the
key directory. The private key is mode-locked to `0600` on Unix.

## Trust model

To verify a signed binary, copy the `.pub` file into the trusted keys directory
(default `~/.daedalus/trusted-keys/`):

```bash
cp ~/.daedalus/keys/<fingerprint>.pub ~/.daedalus/trusted-keys/
```

Then run `daedalus verify my-app.daedalus`.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Signed successfully |
| `1` | Key not found, file error, or signature failure |
