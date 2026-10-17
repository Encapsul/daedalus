# daedalus

[![CI](https://img.shields.io/github/actions/workflow/status/Encapsul/daedalus/ci.yml?branch=main&label=build)](https://github.com/Encapsul/daedalus/actions)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.7.1-green.svg)](https://github.com/Encapsul/daedalus/releases)
[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/artifact-linux%20ELF-lightgrey.svg)]()
[![Runtimes](https://img.shields.io/badge/runtimes%20detected-20-purple.svg)](#supported-runtimes)

## What it is

daedalus packages an app into one self-extracting Linux ELF. The file carries
a launcher, the app, and a runtime when the build embedded one. On Linux you
run that file. The CI smoke that proves this is `examples/hello-web`: a Python
standard-library HTTP server, built and curled on every Linux run.

The CLI itself builds on Linux, macOS, and Windows. The artifact does not.
Running it on macOS or Windows takes WSL or a Linux VM.

On every launch the stub reads the footer, checks SHA-256, extracts into
`~/.cache/daedalus/<hash>/`, then execs the entrypoint. That extract is why
the first start is slower than AppImage or pkg. One measured pass of a 312-byte
Node server (`benchmarks/comparison/results/codespaces-ba69ea`, Node 24):

| Packager | Artifact | On disk after launch | Cold start |
|----------|----------|----------------------|------------|
| AppImage | 24.4 MiB | 24.4 MiB | 29 ms |
| pkg | 70.2 MiB | 70.2 MiB | 109 ms |
| daedalus | 44.0 MiB | 122.6 MiB | 2.1 s |
| Docker | 76.7 MiB | 76.7 MiB | 6.1 s |

daedalus warm start on that pass was 96 ms. Docker needs a daemon. Mounting a
SquashFS without extracting is not implemented. Details and how to reproduce:
[benchmarks/comparison](benchmarks/comparison/README.md).

Signing is on by default with a dev key generated on the build machine and
trusted there. `daedalus verify` on that same machine succeeds because of
that auto-trust. Another machine trusts the file only after `daedalus trust`
of the public key. Optional AES-256-GCM keeps its decryption key in the file
metadata so the stub can decrypt. The Ed25519 private key is not in the file.

SISR can ship a delta instead of the whole file. The "~90% smaller" number in
older notes is a synthetic 200 MiB blob with 10% of its bytes flipped. A bench
on a real pair of model weights exists and is marked `--ignored`.

Names: the command is `daedalus`. The crates.io and PyPI packages are
`daedalux` because `daedalus` was already taken. `daedalus build` writes
`app.de` unless `-o` says otherwise. `scan`, `publish`, and `registry` accept
`.de` and `.daedalus`.

## Quick start

```bash
# Install - any one of these. The shell installer checks sha256 and refuses
# to continue if checksums.txt has no matching line.
cargo install daedalux
brew install encapsul/daedalus/daedalus
pip install daedalux
curl -fsSL https://raw.githubusercontent.com/Encapsul/daedalus/main/install.sh | sh

# Package the app the CI actually smokes. Default output name is app.de.
# The build copies python3 from this machine into the file.
daedalus build ./examples/hello-web -o hello-web.de

# Run it on Linux. The CI smoke does this on the same runner that has Python.
./hello-web.de
```

`cargo install daedalux` ships only the CLI; on first `daedalus build` it downloads
the matching stub from the GitHub release (SHA-256 verified against `checksums.txt`)
and caches it under `~/.cache/daedalus/stubs/`. The Homebrew tap and PyPI package
ship the CLI, stub, and crypto tools together.

## Supported runtimes

Detection order matches `Runtime` in
[`daedalus-core/src/detect.rs`](daedalus-core/src/detect.rs). Keep the two in
sync. "Detected" means the builder recognizes the project. It does not mean a
framework listed in the third column has a passing end-to-end test in CI.

Evidence, as of this tree:

- **CI smoke** (build, run, HTTP 200): Python standard library, `examples/hello-web`, on the Linux workflow. The macOS and Windows workflows build the same example.
- **Workspace e2e, when the toolchain exists**: Rust (`daedalus-cli/tests/rust_build.rs`, cargo is on the CI image). Java/Gradle (`java_build.rs`) skips unless `java` and Gradle ≥ 7 are installed. CI does not install them.
- **Source in `examples/`, not the CI smoke**: `hello-node`, `bottle-web`, `bottle-web-pip`, `hello-zig`, `hello-dart`, `hello-lua`.
- **Scaffold**: `examples/offline-health-agri` is a standard-library script plus a `Modelfile`. It does not contain model weights. If Ollama is already running it queries `localhost:11434`. Otherwise it prints the prompt it would have sent.

| Runtime | Detection | What the code looks for | Evidence |
|---------|-----------|-------------------------|----------|
| Python | `requirements.txt`, `pyproject.toml`, `Pipfile` | Django, FastAPI, Flask, Streamlit markers | CI smoke is stdlib HTTP only |
| Deno | `deno.json`, `deno.jsonc` | Fresh | Detection |
| Node.js | `package.json` | Next.js, Express, NestJS, Fastify, Hono | Example + one comparison bench |
| Electron | `package.json` with `electron` dep | Generic Electron app | Detection |
| Flutter | `pubspec.yaml` with `flutter` SDK dep | Release bundle from `lib/main.dart` | Needs the Flutter SDK on PATH. No SDK download |
| Dart | `pubspec.yaml` without the `flutter` SDK dep | AOT-compiled `bin/main.dart` | Example in repo |
| Java | `pom.xml`, `build.gradle` | Spring Boot | e2e test, skipped without java+gradle |
| Ruby | `Gemfile`, `_config.yml` | Rails, Sinatra, Jekyll | Detection |
| .NET/C# | `*.csproj` | ASP.NET | Detection |
| Rust | `Cargo.toml` | Static binary | Workspace e2e |
| Zig | `build.zig`, `build.zig.zon` | `zig build` -> static binary | Example in repo |
| Go | `go.mod` | Static binary | Detection |
| PHP | `composer.json` | Laravel, Symfony, WordPress | Detection |
| Perl | `Makefile.PL`, `cpanfile` | Mojolicious layout only | Detection |
| Lua | `*.lua`, `lua` in package.json | OpenResty, Neovim plugins | Example in repo |
| Hugo | `hugo.toml`, `config.toml` | Static sites | Detection |
| Ollama | `Modelfile`, `models/*.gguf`, `ollama` in `package.json`, or `DAEDALUS_OLLAMA=1` | `ollama serve` | Detection. Weights are whatever is in the project |
| Gemma | `Modelfile` with a Gemma `FROM`, or `models/gemma*.gguf` | `ollama run <model-id>` | Detection. The in-repo example has no weights |
| Wasm | `*.wasm` | WASI (wasmtime) | Detection |
| Binary | ELF/PE executable | Any native binary | Detection |

## Binary format

```
[stub][payload][metadata][footer]
```

- **Stub**: statically-linked launcher (Linux ELF; musl, so it owns no dynamic
  dependencies - the only host dependency is the Linux kernel). It reads its own
  binary, verifies integrity, extracts the payload, and `execvp`s the entrypoint
- **Payload**: zstd-compressed tar archive (or squashfs) of the application + runtime
- **Metadata**: JSON with runtime info, entrypoint, layers, capabilities
- **Footer**: magic `0xBEEF_CAFE`, format magic `DAE\x01`, format version, integrity SHA-256 hash

Format versions: v2 (plain), v3 (signed), v4 (encrypted), v5 (squashfs).

## CLI commands

| Command | Description |
|---------|-------------|
| `build <dir>` | Package an app directory into a `.de` file |
| `run <file>` | Execute a `.de` file |
| `inspect <file>` | Read metadata from a `.de` file |
| `scan [dir]` | Find `.de` files and display metadata |
| `sign <file>` | Sign a `.de` with an Ed25519 private key |
| `verify <file>` | Verify the signature against trusted keys |
| `keygen` | Generate an Ed25519 keypair |
| `trust <keyfile>` | Add a public key to trusted keys |
| `doctor` | Check system prerequisites |
| `clean` | Remove daedalus cache and build artifacts |
| `selftest <file>` | Test a `.de` file in an ephemeral sandbox |
| `upgrade` | Self-update the daedalus binary |
| `migrate <input> <output>` | Migrate legacy v1 binary to SISR-enabled v2 |
| `swap <binary> <layer> <file>` | Hot-swap a layer in a `.de` binary |
| `publish <file>` | Publish a `.de` file to a registry |
| `registry push|pull|list` | Push/pull/list layers in a content-addressable registry |
| `env` | Show daedalus environment info |
| `completion <shell>` | Generate shell completions |
| `man [dir]` | Generate man pages |

## Global flags

| Flag | Description |
|------|-------------|
| `--verbose` / `-v` | Enable verbose output |
| `--quiet` / `-q` | Suppress non-error output |
| `--no-color` | Disable colored output |
| `--plain` | Machine-readable output (no ANSI, no pager, no box drawing) |
| `--no-input` | Disable all interactive prompts (for CI/scripts) |
| `--json` | Output as JSON (where supported) |

## Common options

| Flag | Commands | Description |
|------|----------|-------------|
| `--target` | build | Cross-compile target. CI-verified (stub built + smoke-tested): `linux-x64`, `linux-arm64`, `darwin-x64`, `darwin-arm64`, `win-x64`. `win-arm64` is accepted but not built by CI. Any Rust triple is parsed; the short forms are conveniences. |
| `--sign --key <file>` | build | Sign the binary with Ed25519 |
| `--encrypt <keyfile>` | build | Encrypt payload with AES-256-GCM |
| `--enable-sisr --update-url <url>` | build | Enable delta updates |
| `--dry-run` | build, scan | Preview without doing anything |
| `--output` / `-o` | build, inspect, scan | Output file/directory |
| `--strict` | doctor | Exit with error if any check fails |
| `--force` | clean, sign, migrate | Skip confirmation prompts |
| `--local <dir>` | registry | Use local directory instead of HTTP registry |

## Build from source

### Prerequisites

- Rust toolchain (stable, 2021 edition): <https://rustup.rs>
- C compiler (`gcc` or `clang`)
- On Linux: `musl-tools` for static linking
  ```bash
  # Ubuntu/Debian
  sudo apt install musl-tools gcc
  rustup target add x86_64-unknown-linux-musl

  # Fedora
  sudo dnf install musl-gcc
  rustup target add x86_64-unknown-linux-musl
  ```

### Build

```bash
git clone https://github.com/Encapsul/daedalus.git
cd daedalus
cargo build --release
# Binary at target/release/daedalus
```

### Workspace

| Crate | Purpose |
|-------|---------|
| `daedalux-core` | Shared library: format, compression, detection, signing, assembly (published crates.io as `daedalux-core`) |
| `daedalus-stub` | Self-extracting launcher (Linux ELF only today; not published to crates.io) |
| `daedalux` | CLI tool (cross-platform; published crates.io as `daedalux`, installs the `daedalus` binary) |

## Configuration

Place a `.daedalus.toml` in your app directory. CLI flags override config file values.

```toml
[package]
version = "1.0.0"
author = "Your Name"
description = "My awesome app"

[build]
isolation = "sandbox"
seccomp = true
target = "x86_64"
no_install = false
env_file = ".env"
```

## Security

Give another machine the public key. Verification on the build machine is not
that proof: the default dev key is written into that machine's trust store
during `daedalus build`.

```bash
daedalus keygen --key-dir ./keys
daedalus build ./examples/hello-web -o hello-web.de --key ./keys/<id>.key
# on the other machine, with only the .pub file:
daedalus trust ./keys/<id>.pub
daedalus verify hello-web.de
```

`daedalus verify` with an empty trust directory fails. That is the check you
want before you hand the file to someone else.

- **Ed25519**: strict verification (`verify_strict`, ZIP-215). Signing is on
  by default. `--skip-sign` opts out. `--key` selects the key.
- **SHA-256**: the stub checks the payload hash before extraction. Release
  downloads (stub, upgrade, Homebrew, PyPI, `install.sh`) are checked against
  `checksums.txt`. `scripts/install.sh` is the older installer. It now refuses
  to install when that check cannot run. Prefer the root `install.sh`.
- **AES-256-GCM**: optional (`--encrypt`). The decryption material is in the
  file metadata, because the stub has to decrypt. It stops a casual `tar` of
  the payload. It does not hide the app from whoever can run the file.
- **Isolation**: on Linux, user and mount namespaces plus `pivot_root` when
  the kernel allows it. seccomp is a denylist of 18 syscalls (`ptrace`, `bpf`,
  `userfaultfd`, `mount`, module loading, `reboot`, …). Networking, file I/O,
  and `exec` stay available unless capabilities say otherwise. This is not an
  application sandbox. If seccomp cannot be installed, the launcher warns and
  continues. macOS has an App Sandbox profile path. Windows has process
  isolation. See [docs/src/security.md](docs/src/security.md).
- **`selftest`**: runs the file from a temporary directory so you can see it
  start before you keep it.
- **SISR**: a delta is applied only when its Ed25519 signature checks.
- **SBOM**: `daedalus inspect --sbom` lists the recorded contents.
- **Attestations**: `daedalus attest app.de` signs an in-toto statement
  over the SBOM and the SHA-256 of the whole file, stub included.
  `daedalus attest app.de --verify` checks that statement against the
  local trust store.

### Attestation threat model

What `daedalus attest` actually guarantees, and what it does not:

- **Covers the SBOM.** The signature is computed over a canonical envelope of
  `(file digest, SBOM)`, so changing a version string in the SBOM invalidates the
  signature. An attacker cannot substitute a false inventory.
- **Covers the whole file, stub included.** The digest is over every byte of the
  `.de`, not just payload and metadata, so a substituted launcher is detected.
  (`daedalus sign` signs `payload || metadata || footer` only; the attestation is
  deliberately broader.)
- **Strict verification.** `verify_strict` rejects small-order keys and non-canonical
  signatures per ZIP-215.
- **Offline.** Verification needs only a trusted public key. No CA, no network, no
  transparency log, no clock.
- **Not a timestamp or a transparency guarantee.** An attestation says who signed
  *what bytes*; it does not prove the key was not compromised, nor that no other
  valid signature exists for different bytes. There is no revocation list. If you
  need key lifecycle, manage keys outside the tool.

Tests that enforce the above: `signature_covers_sbom_so_tampering_breaks_verification`,
`file_digest_covers_the_stub_region`, `strict_verification_rejects_small_order_key_zip215`
in `daedalus-cli/src/commands/attest.rs`.

Trust model: distributes through the same channels auditors already trust
(crates.io, Homebrew, PyPI) rather than piping random binaries, and ships the
verify/sign/trust story built in, not as an afterthought.

## Development

```bash
# Format check
cargo fmt --check

# Lint (per-crate, see AGENTS.md)
cargo clippy -p daedalux-core --all-targets -- -D warnings
cargo clippy -p daedalus-stub --all-targets -- -D warnings
cargo clippy -p daedalux --all-targets -- -D warnings

# Tests
cargo test --workspace
```

See [CODE_STYLE.md](CODE_STYLE.md) for Rust style guidelines and [AGENTS.md](AGENTS.md) for build constraints.

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feat/my-feature`)
3. Commit with signed commits (`git commit -S -m "feat: ..."`)
4. Push and open a pull request

## License

MIT - see [LICENSE](LICENSE) for details.

## Community

- [GitHub Issues](https://github.com/Encapsul/daedalus/issues) - report bugs or request features
- `daedalus feedback --browser` - open the feedback page quickly