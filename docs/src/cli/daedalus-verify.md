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

A build machine already has its dev key there, so `daedalus verify` on that
machine succeeds without an extra step. That check does not travel with the
file. On any other machine the trust directory starts empty and verify fails
until you install the publisher's public key.

```bash
# Publisher
daedalus keygen --key-dir ./keys
daedalus build ./examples/hello-web -o hello-web.daedalus --key ./keys/<fingerprint>.key

# Recipient, after receiving hello-web.daedalus and the .pub only
daedalus trust ./keys/<fingerprint>.pub
daedalus verify hello-web.daedalus
```

All `.pub` files in the trust directory are tried. Verification succeeds if
one of them matches. `--trusted-dir` points somewhere else.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Verified successfully |
| `1` | Integrity failure, missing signature, or no trusted key matched |
