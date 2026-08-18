# erebus

[![CI](https://img.shields.io/github/actions/workflow/status/Tednoob17/erebus/ci.yml?branch=main&label=build)](https://github.com/Tednoob17/erebus/actions)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.5.0-green.svg)](https://github.com/Tednoob17/erebus/releases)
[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-linux%20%7C%20macos%20%7C%20windows-lightgrey.svg)]()
[![Runtimes](https://img.shields.io/badge/runtimes-11-purple.svg)](#supported-runtimes)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](https://github.com/Tednoob17/erebus/pulls)

**Package any app into a single self-extracting binary.**

erebus compiles any web, server, or CLI application into a single self-contained executable. Supported runtimes: Python, Node.js, Deno, Java, Ruby, .NET/C#, Go, PHP, Perl, Binary, Hugo.

The binary format (`[stub][payload][metadata][footer]`) is a universal executable artifact — capable of transporting an application, an AI agent, a microservice, or a plugin as a single portable unit.

## Quick start

```bash
curl -fsSL https://raw.githubusercontent.com/Tednoob17/erebus/main/scripts/install.sh | bash
erebus doctor
cd your-app && erebus build . -o myapp.ere
./myapp.ere
```

## Overview

erebus transforms any application directory into a portable, self-extracting binary that can run on target machines without requiring the host runtime. This includes:

- **Python applications** — Django, FastAPI, Flask
- **Node.js applications** — Next.js, Express, Fastify
- **Go binaries** — Standalone executables
- **Namespace isolation** — user/mount namespaces + optional seccomp (not full Docker sandboxing)

The tool handles runtime detection, dependency installation, compression, and signing through a unified, language-agnostic pipeline.

## Supported runtimes

| Runtime | Detection | Framework support |
|---------|-----------|-------------------|
| Python | `requirements.txt`, `pyproject.toml`, `Pipfile` | Django, FastAPI, Flask |
| Node.js | `package.json` | Next.js, Nuxt, Astro, Remix, SvelteKit, Express, Fastify, Hono |
| Deno | `deno.json`, `deno.jsonc` | Fresh |
| Java | `pom.xml`, `build.gradle` | Spring Boot |
| Ruby | `Gemfile` | Sinatra, Rails |
| .NET/C# | `*.csproj`, `*.sln` | ASP.NET |
| Go | `go.mod` | Static binary |
| PHP | `composer.json` | Laravel, Symfony, WordPress |
| Perl | `Makefile.PL`, `cpanfile` | Mojolicious, Dancer |
| Hugo | `hugo.toml`, `hugo.yaml` | Static site generator |
| Binary | ELF executable | Any static or dynamic binary |

Each runtime has specific detection logic and framework support that triggers when building.

## Binary format

The `.ere` binary is structured as:

```
[stub][payload][metadata][footer]
```

- **Stub**: statically-linked launcher (musl ELF on Linux) that reads its own binary, verifies integrity, extracts the payload, and `execvp`s the entrypoint
- **Payload**: zstd-compressed tar archive (or squashfs) of the application + runtime
- **Metadata**: JSON with runtime info, entrypoint, layers, capabilities, version
- **Footer**: 4-byte magic `0xBEEF_CAFE`, format version, integrity SHA-256 hash

Format versions: v2 (plain), v3 (signed), v4 (encrypted), v5 (squashfs).

## Target & host support

`erebus build --target <list>` packages one artifact **per target**. A comma-separated
list emits one `<name>-<target>.ere` per target into the output directory. The
stubs and (where available) embedded runtimes are cross-selected by target.

| `--target` short form | Resolved triple | Stub | Runtime arch | Notes |
|-----------------------|-----------------|------|--------------|-------|
| *(none)* / `host`     | host triple     | host stub | host     | Native build |
| `linux-x64`           | `x86_64-unknown-linux-musl` | ELF (static) | node x64 linux | |
| `linux-arm64`         | `aarch64-unknown-linux-musl` | ELF (static) | node arm64 linux | |
| `darwin-x64`          | `x86_64-apple-darwin` | Mach-O | node x64 darwin | macOS host runs this |
| `darwin-arm64`        | `aarch64-apple-darwin` | Mach-O | node arm64 darwin | native Apple Silicon |
| `win-x64`             | `x86_64-pc-windows-gnu` | PE (.exe) | node x64 win | cross-OS from any host |
| `win-arm64`           | `aarch64-pc-windows-gnu` | PE (.exe) | node arm64 win | cross-OS from any host |

- The builder host can be any supported OS (Linux/macOS/Windows). Cross-OS packaging
  (e.g. building a Windows `.exe` on Linux) bundles the matching stub + runtime;
  the produced artifact runs only on its target OS.
- Full triples (`aarch64-apple-darwin`, `x86_64-unknown-linux-musl`) are also accepted.

## CLI commands

| Command | Description |
|---------|-------------|
| `erebus build <dir>` | Package an app directory into a `.ere` file |
| `erebus inspect <file>` | Read metadata from a `.ere` file |
| `erebus scan [dir]` | Find `.ere` files recursively and display metadata |
| `erebus sign <file>` | Sign a `.ere` with an Ed25519 private key |
| `erebus verify <file>` | Verify the signature of a `.ere` against trusted keys |
| `erebus keygen` | Generate an Ed25519 keypair for signing |
| `erebus trust <keyfile>` | Add a public key to the trusted keys directory |
| `erebus doctor` | Check system prerequisites and report missing dependencies |
| `erebus upgrade` | Self-update the erebus binary |
| `erebus env` | Show erebus environment and build configuration |
| `erebus clean` | Remove erebus cache and build artifacts |
| `erebus completion <shell>` | Generate shell completions for bash, zsh, fish, elvish, or powershell |
| `erebus man [dir]` | Generate man pages to the specified directory |

### Key features
- **Global `--verbose`** flag for detailed output on any command
- **`--strict`** mode on `erebus doctor` for strict validation
- **`--dry-run`** flag on build, inspect, scan for preview operations
- **`.ere.toml`** configuration file support
- **Rust-based CLI** with zero Python dependency at runtime
- **Self-updating** via `erebus upgrade`

### Advanced build options

```bash
# Basic build
erebus build ./myapp -o myapp.ere

# Embed interpreter for self-contained binaries
erebus build ./myapp --embed-interpreter python3 -o myapp.ere

# Multi-arch packaging: one artifact per target
erebus build ./myapp --target linux-x64,linux-arm64 -o out/app.ere
#  -> out/app-linux-x64.ere, out/app-linux-arm64.ere

# Cross-OS packaging: Windows artifact built on Linux/macOS
erebus build ./myapp --target win-x64 -o out/app.ere
#  -> out/app-win-x64.exe (stubs+runtime selected for the target OS)

# Sign and encrypt
erebus keygen --key-dir ~/.ere/keys
erebus build ./myapp --sign --key ~/.ere/keys/*.key -o myapp.ere
erebus build ./myapp --encrypt --key ~/.ere/keys/*.key -o myapp-secure.ere

# Self-updating binary with SISR (delta updates)
erebus build ./myapp --enable-sisr --update-url https://updates.example.com -o myapp.ere
#  -> myapp.ere + myapp.ere.manifest

# Persistent storage
erebus build ./myapp --persist -o myapp.ere
#  -> ERE_PERSIST_DIR injected into app environment

# Environment injection
erebus build ./myapp --env-file .env --env KEY=VALUE -o myapp.ere
```

### Embedded Runtime Options

The `--embed-interpreter` flag bundles an interpreter into the binary:

| Option | Description |
|--------|-------------|
| `python3` | Embed Python 3 interpreter |
| `node` | Embed Node.js runtime |
| `deno` | Embed Deno runtime |
| `ruby` | Embed Ruby interpreter |
| `php` | Embed PHP interpreter |
| `perl` | Embed Perl interpreter |
| `java` | Embed Java runtime |
| `go` | Embed Go runtime |

### Build Cache

Intelligent caching skips extraction when the app hash matches:

```bash
# Use cache for faster rebuilds
erebus build ./myapp --use-cache

# Clear cache before building
erebus build ./myapp --clear-cache
```

## Support matrix

| Host → Target | linux-x64 | linux-arm64 | macos-x64 | macos-arm64 | win-x64 | win-arm64 |
|--------------|-----------|-------------|-----------|-------------|---------|-----------|
| linux-x64    | native | cross    | cross  | cross    | cross | cross |
| linux-arm64  | cross  | native   | cross  | cross    | cross | cross |
| macos-x64    | cross  | cross    | native | cross    | cross | cross |
| macos-arm64  | cross  | cross    | cross  | native   | cross | cross |
| win-x64      | —      | —        | —      | —        | native | cross |
| win-arm64    | —      | —        | —      | —        | cross | native |

- Cross-OS builds embed the target stub + runtime; the artifact runs only on its target OS
- Cross-arch builds require `cargo zigbuild` (zig as cc/linker) for the stub; the CLI itself builds with standard `cargo`

## Configuration

Place a `.ere.toml` in your app directory. CLI flags override config file values.

```toml
[package]
version = "1.0.0"
author = "Your Name"
description = "My awesome app"
license = "MIT"

[build]
isolation = "sandbox"
seccomp = true
encrypt = false
squashfs = false
target = "x86_64"
no_install = false
env_file = ".env"
```

### Shell completion

```bash
# Bash
erebus completion bash >> ~/.bashrc

# Zsh
erebus completion zsh >> ~/.zshrc

# Fish
erebus completion fish > ~/.config/fish/completions/erebus.fish
```

### Man pages

```bash
erebus man /usr/local/share/man/man1/
```

## Build from source

Requires Rust toolchain and `cargo`.

```bash
git clone https://github.com/Tednoob17/erebus.git
cd erebus
cargo build --release
# Binary at target/release/erebus
```

### Cargo workspace

| Crate | Purpose |
|-------|---------|
| `erebus-core` | Shared library: format, compression, detection, signing, assembly, CAS, SISR |
| `erebus-stub` | Self-extracting launcher (Linux ELF / macOS Mach-O / Windows PE) |
| `erebus-cli` | CLI tool (cross-platform) |

## Security

- **Ed25519 signing**: binaries can be signed and verified against trusted keys
- **Encryption**: AES-256-GCM payload encryption with key derivation
- **SHA-256 integrity**: footer hash verifies payload tampering at runtime
- **Namespace isolation**: Linux builds use user/mount namespaces + optional seccomp
- **CVE-2023-48022**: ed25519-dalek pinned to >= 2.1.0 to ensure Ed25519 bit is set

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feat/my-feature`)
3. Commit with signed commits (`git commit -S -m "feat: ..."`)
4. Push and open a pull request

Run checks before submitting:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

### Coding conventions

See [CODE_STYLE.md](CODE_STYLE.md) for Rust style guidelines, best practices, and formatting rules.

## License

MIT — see [LICENSE](LICENSE) for details.
