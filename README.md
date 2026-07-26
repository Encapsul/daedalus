# x.bin

[![CI](https://img.shields.io/github/actions/workflow/status/Tednoob17/x.bin/ci.yml?branch=main&label=build)](https://github.com/Tednoob17/x.bin/actions)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.3.2-green.svg)](https://github.com/Tednoob17/x.bin/releases)
[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-linux%20%7C%20macos-lightgrey.svg)]()
[![Runtimes](https://img.shields.io/badge/runtimes-11-purple.svg)](#supported-runtimes)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](https://github.com/Tednoob17/x.bin/pulls)

Package any app into a single self-extracting binary.

x.bin compiles any web, server, or CLI application into a single self-contained ELF executable. Supported runtimes: Python, Node.js, Deno, Java, Ruby, .NET/C#, Go, PHP, Perl, Binary, Hugo.

## Quick start

```bash
curl -fsSL https://raw.githubusercontent.com/Tednoob17/x.bin/main/scripts/install.sh | bash
xbin doctor
cd your-app && xbin build . -o myapp.xbin
./myapp.xbin
```

## Overview

x.bin transforms any application directory into a portable, self-extracting binary that can run on target machines without requiring the host runtime. This includes:

- **Python applications** — Django, FastAPI, Flask
- **Node.js applications** — Next.js, Express, Fastify
- **Go binaries** — Standalone executables
- **Docker-like isolation** — Sandboxed, portable execution

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

## CLI commands

| Command | Description |
|---------|-------------|
| `xbin build <dir>` | Package an app directory into a `.xbin` file |
| `xbin inspect <file>` | Read metadata from a `.xbin` file |
| `xbin scan [dir]` | Find `.xbin` files recursively and display metadata |
| `xbin sign <file>` | Sign a `.xbin` with an Ed25519 private key |
| `xbin verify <file>` | Verify the signature of a `.xbin` against trusted keys |
| `xbin keygen` | Generate an Ed25519 keypair for signing |
| `xbin trust <keyfile>` | Add a public key to the trusted keys directory |
| `xbin doctor` | Check system prerequisites and report missing dependencies |
| `xbin env` | Show xbin environment and build configuration |
| `xbin clean` | Remove xbin cache and build artifacts |
| `xbin completion <shell>` | Generate shell completions for bash, zsh, fish, elvish, or powershell |
| `xbin man [dir]` | Generate man pages to the specified directory |

### Key features
- **Global `--verbose`** flag for detailed output on any command
- **`--strict`** mode on `xbin doctor` for strict validation
- **`--dry-run`** flag on build, inspect, scan for preview operations
- **`.xbin.toml`** configuration file support
- **Rust-based CLI** with zero Python dependency at runtime

### Advanced build options

```bash
# Embed interpreter for self-contained binaries (Bun-inspired)
xbin build ./myapp --embed-interpreter python3 -o myapp.xbin

# Enable WASM support (Wasmer-inspired)
xbin build ./myapp --wasm --wasmtime-path /usr/bin/wasmtime -o myapp.xbin

# Cross-compilation (Bun-inspired)
xbin build ./myapp --cross-compile aarch64,arm64 -o myapp.xbin

# HTTP health check endpoint (Wasmer-inspired)
xbin build ./myapp --health-port 8080 --health-endpoint /health -o myapp.xbin

# Intelligent build caching (Wasmer-inspired)
xbin build ./myapp --use-cache --clear-cache -o myapp.xbin
```

#### Embedded Runtime Options

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
| `wasm` | Embed WASM runtime |
| `custom` | Use custom interpreter path (requires `--interpreter-path`) |

#### Build Cache

Intelligent caching skips extraction when the app hash matches:

```bash
# Use cache for faster rebuilds
xbin build ./myapp --use-cache

# Clear cache before building
xbin build ./myapp --clear-cache
```

## Configuration

Place a `.xbin.toml` in your app directory. CLI flags override config file values.

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

### Build flags

```bash
xbin build ./myapp \
  --output myapp.xbin \
  --target aarch64 \
  --squashfs \
  --encrypt \
  --env-file .env \
  --dry-run \
  --verbose
```

### Shell completion

```bash
# Bash
xbin completion bash >> ~/.bashrc

# Zsh
xbin completion zsh >> ~/.zshrc

# Fish
xbin completion fish > ~/.config/fish/completions/xbin.fish
```

### Man pages

```bash
xbin man /usr/local/share/man/man1/
```

## Build from source

Requires Rust toolchain and `cargo`.

```bash
git clone https://github.com/Tednoob17/x.bin.git
cd x.bin
cargo build --release
# Binary at target/release/xbin
```

### Cargo workspace

| Crate | Purpose |
|-------|---------|
| `xbin-core` | Shared library: format, compression, detection, signing, assembly |
| `stub` | Self-extracting launcher (Linux ELF) |
| `xbin-cli` | CLI tool (cross-platform) |

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feat/my-feature`)
3. Commit with signed commits (`git commit -S -m "feat: ..."`)
4. Push and open a pull request

Run checks before submitting:

```bash
cargo fmt --check
cargo clippy -p xbin-core --all-targets -- -D warnings
cargo test --workspace
```

### Coding conventions

See [CODE_STYLE.md](CODE_STYLE.md) for Rust and Python style guidelines, best practices, and formatting rules.

## License

MIT — see [LICENSE](LICENSE) for details.