# daedalus

[![CI](https://img.shields.io/github/actions/workflow/status/Encapsul/daedalus/ci.yml?branch=main&label=build)](https://github.com/Encapsul/daedalus/actions)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.5.0-green.svg)](https://github.com/Encapsul/daedalus/releases)
[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-linux%20%7C%20macos%20%7C%20windows-lightgrey.svg)]()
[![Runtimes](https://img.shields.io/badge/runtimes-11-purple.svg)](#supported-runtimes)

## AI model packaging (Gemma via Ollama, on-device)

daedalus compiles any web, server, or CLI application into a single self-contained executable **and** can package local AI models (Gemma via Ollama, Llama.cpp, etc.) embedded in the binary, running entirely offline — no cloud, no GPU, zero runtime cost. See [AI and edge runtimes](docs/src/guides/ai-edge.md).

## AI anywhere

daedalus is the deployment layer for local AI. Package Ollama + any local model + your app in one `.de` binary. Deploy AI in low-connectivity environments, on-premise, or air-gapped. No cloud dependency, no GPU required. Update models via SISR delta updates over 960kbps links.

The binary format (`[stub][payload][metadata][footer]`) is a universal executable artifact — capable of transporting any application, microservice, or plugin as a single portable unit.

---

**Note**: the produced `.de` file is a Linux ELF binary. It runs natively on Linux, and can be run on macOS/Windows via WSL or a Linux VM. Building on Windows/macOS works (the CLI is cross-platform), but the output requires a Linux runtime to execute.

## Quick start

```bash
# Install
curl -fsSL https://raw.githubusercontent.com/Encapsul/daedalus/main/scripts/install.sh | bash

# Build a Python app with Gemma embedded
cd your-app && daedalus build . -o myapp.daedalus

# Run — Ollama auto-starts, Gemma model loads locally
./myapp.daedalus
```

## Supported runtimes

| Runtime | Detection | Frameworks |
|---------|-----------|------------|
| Python | `requirements.txt`, `pyproject.toml`, `Pipfile` | Django, FastAPI, Flask, Streamlit |
| Node.js | `package.json` | Next.js, Express, NestJS, Fastify, Hono |
| Deno | `deno.json`, `deno.jsonc` | Fresh |
| Electron | `package.json` with `electron` dep | Generic Electron app |
| Java | `pom.xml`, `build.gradle` | Spring Boot |
| Ruby | `Gemfile`, `_config.yml` | Rails, Sinatra, Jekyll |
| .NET/C# | `*.csproj` | ASP.NET |
| Go | `go.mod` | Static binary |
| PHP | `composer.json` | Laravel, Symfony, WordPress |
| Perl | `Makefile.PL`, `cpanfile` | Mojolicious |
| Hugo | `hugo.toml`, `config.toml` | Static sites |
| Rust | `Cargo.toml` | Static binary |
| Wasm | `*.wasm` | WASI (wasmtime) |
| Binary | ELF/PE executable | Any native binary |

## Binary format

```
[stub][payload][metadata][footer]
```

- **Stub**: statically-linked launcher (currently Linux ELF only; macOS/Windows PE support is planned) that reads its own binary, verifies integrity, extracts the payload, and `execvp`s the entrypoint
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
| `--target` | build | Cross-compile target (`linux-x64`, `linux-arm64`, `darwin-x64`, `darwin-arm64`, `win-x64`, `win-arm64`) |
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
| `daedalus-core` | Shared library: format, compression, detection, signing, assembly |
| `daedalus-stub` | Self-extracting launcher (Linux ELF only today; macOS/Windows PE planned) |
| `daedalus-cli` | CLI tool (cross-platform) |

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

- **Ed25519 signing**: binaries can be signed and verified against trusted keys
- **SHA-256 integrity**: footer hash verifies payload tampering at runtime
- **AES-256-GCM encryption**: optional payload encryption with external key (`--encrypt` / `--decrypt-key`)
- **Namespace isolation**: user/mount namespaces + optional seccomp on Linux; App Sandbox on macOS; process isolation on Windows
- **Delta updates (SISR)**: the stub verifies an embedded Ed25519 signature at cold start

## Development

```bash
# Format check
cargo fmt --check

# Lint (per-crate, see AGENTS.md)
cargo clippy -p daedalus-core --all-targets -- -D warnings
cargo clippy -p daedalus-stub --all-targets -- -D warnings
cargo clippy -p daedalus-cli --all-targets -- -D warnings

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

MIT — see [LICENSE](LICENSE) for details.

## Community

- [GitHub Issues](https://github.com/Encapsul/daedalus/issues) — report bugs or request features
- `daedalus feedback --browser` — open the feedback page quickly