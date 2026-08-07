# `xbin build`

Package an app directory into a single self-extracting `.xbin` executable.

```bash
xbin build [OPTIONS] [APP]
```

`APP` defaults to `.`. The output is `app.xbin` unless `-o` is given (a
trailing slash appends `app.xbin` to the directory).

## Runtime selection

The runtime is auto-detected from the app directory (Python, Node.js, Deno,
Java, Ruby, .NET/C#, Go, PHP, Perl, native binary). Override the detected
interpreter with `--embed-interpreter` (for example `python3`, `node`,
`php`, `ruby`, `deno`).

## Common options

| Flag | Description |
|---|---|
| `-o, --output <PATH>` | Output file (default `app.xbin`) |
| `-k, --key <PATH>` | Ed25519 signing key (32 raw bytes) |
| `--isolation <MODE>` | Isolation level: `sandbox` (0) .. hard (default `sandbox`) |
| `--seccomp` | Install a seccomp BPF denylist at runtime |
| `--landlock` | Enable Landlock LSM filesystem sandbox |
| `--encrypt` | Encrypt the payload (AES-256-GCM, key derived from signing seed) |
| `--squashfs` | Use SquashFS instead of zstd+tar for the payload |
| `--no-install` | Skip dependency installation |
| `--env-file <PATH>` | Bake in a `KEY=VALUE` env file |
| `--env KEY=VALUE` | Set an env var (repeatable) |
| `--define KEY=VALUE` | Build-time define, injected as an env var (repeatable) |
| `--version-info <STR>` | Version string recorded in metadata |
| `--include <PATH>` | Extra files/dirs to embed in the rootfs (repeatable) |
| `--dry-run` | Print the build plan without building |
| `--json` | Emit a JSON build result on stdout |
| `-v/--quiet` | Verbose / quiet output |

## SISR options (incremental self-updates)

| Flag | Description |
|---|---|
| `--enable-sisr` | Enable delta-indexing: content-chunk the payload, embed a SISR section and write `<output>.manifest` |
| `--key <PATH>` | **With `--enable-sisr`:** signs the SISR manifest instead of the binary |
| `--update-url <URL>` | Base URL of the update channel; embedded in the binary and used by `--xbin-update` |

```bash
# Updatable binary: SISR section + signed manifest + embedded update channel
xbin build ./my_app -o my_app.xbin \
    --enable-sisr \
    --key ~/.xbin/keys/<fingerprint>.key \
    --update-url https://updates.example.com/my_app
```

This produces two artifacts:

- `my_app.xbin` — the self-extracting binary (payload is content-addressed;
  unchanged chunks can be reused in place during an update);
- `my_app.xbin.manifest` — the signed `XBMR` remote manifest that the launcher
  fetches and verifies before applying an update.

Publish both, plus the chunk files, to the update channel so target machines
can run `./my_app.xbin --xbin-update` (see [User-updates](../guides/user-updates.md)).

### Signing semantics

The Ed25519 key file is the same 32 raw bytes used by `xbin sign`. When
`--enable-sisr` is given, `--key` signs the **manifest** (the SISR footer
carries the signature; the launcher verifies it against your trusted keys).
Because the binary signature block would be inserted after the metadata and
truncate the SISR section, a single `xbin build` signs *either* the binary
(no `--enable-sisr`) *or* the manifest (`--enable-sisr`), never both. The
private key bytes are never printed; under Unix a key file that is not mode
`0600` produces a warning.

## Behavior

1. Detects the runtime and installs dependencies (`--no-install` to skip).
2. Copies the app into a rootfs, optionally embedding an interpreter.
3. Creates a deterministic `zstd+tar` payload (or SquashFS).
4. Builds metadata (env, entrypoint, hashes, `update_url`).
5. Assembles the binary — with `--enable-sisr` the payload is content-chunked
   (64 KiB targets) and the signed manifest is emitted.

## Dry run

`--dry-run` prints the full plan — runtime, isolation, compression, SISR,
package managers, file count — without building anything:

```bash
xbin build ./my_app --enable-sisr --update-url https://… --dry-run
```

## See also

- [`xbin sign`](./xbin-sign.md), [`xbin verify`](./xbin-verify.md),
  [`xbin keygen`](./xbin-keygen.md)
- [Incremental Updates (SISR)](../guides/incremental-updates.md)
- [User-facing update workflow](../guides/user-updates.md)
