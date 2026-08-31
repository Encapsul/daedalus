# `daedalus run`

Execute a `.daedalus` self-extracting binary.

```bash
daedalus run [OPTIONS] <FILE>
```

The launcher (`daedalus-stub`) is appended to the front of the file. At runtime
it:

1. Reads its own footer to locate the payload and metadata.
2. Verifies the `SHA-256` integrity hash.
3. If the binary is signed, verifies the Ed25519 signature against trusted keys.
4. Extracts the payload (zstd+tar or SquashFS) to
   `~/.cache/daedalus/<sha256>/rootfs/`.
5. `execvp`s the entrypoint.

Any arguments after `<FILE>` are forwarded to the app.

```bash
./my-app.daedalus --port 8080
# is equivalent to:
daedalus run my-app.daedalus --port 8080
```

## Options

| Flag | Description |
|---|---|
| `--verbose` | Show extraction, cache hit/miss, and launch details |
| `--daedalus-version` | Print the daedalus version and exit |
| `--daedalus-update <URL>` | Override the embedded update URL for SISR |
| `--decrypt-key <PATH>` | Decrypt an AES-256-GCM encrypted payload |

## Decrypt-key

When the payload was built with `--encrypt`, pass the decryption key at runtime:

```bash
daedalus run secret-app.daedalus --decrypt-key /path/to/keyfile
```

The key is read from disk, used to derive the AES-256-GCM key, and then
zeroized. It is never embedded in the binary.

## Debugging

```bash
DAEDALUS_VERBOSE=1 ./my-app.daedalus
```

Shows cold/warm cache status, extraction path, and entrypoint command.

## Environment

| Variable | Description |
|---|---|
| `DAEDALUS_VERBOSE` | Enable verbose logging (same as `--verbose`) |
| `DAEDALUS_UPDATE_URL` | Override the embedded SISR update URL |
| `DAEDALUS_STUB_PATH` | Path to a custom stub binary (dev only) |

## Exit codes

| Code | Meaning |
|---|---|
| `0` | App exited successfully |
| `1` | Extraction, verification, or launch failure |
| `2` | Invalid arguments |
