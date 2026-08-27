# daedalus

[![CI](https://img.shields.io/github/actions/workflow/status/Tednoob17/daedalus/ci.yml?branch=main&label=build)](https://github.com/Tednoob17/daedalus/actions)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.5.0-green.svg)](https://github.com/Tednoob17/daedalus/releases)
[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-linux%20%7C%20macos%20%7C%20windows-lightgrey.svg)]()
[![Runtimes](https://img.shields.io/badge/runtimes-11-purple.svg)](#supported-runtimes)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](https://github.com/Tednoob17/daedalus/pulls)

**Package any app into a single self-extracting binary.**

daedalus compiles any web, server, or CLI application into a single self-contained executable. Supported runtimes: Python, Node.js, Deno, Java, Ruby, .NET/C#, Go, PHP, Perl, Hugo, Binary.

The binary format (`[stub][payload][metadata][footer]`) is a universal
executable artifact — capable of transporting any application, microservice,
or plugin as a single portable unit.

## Quick start

```bash
curl -fsSL https://raw.githubusercontent.com/Tednoob17/daedalus/main/scripts/install.sh | bash
daedalus doctor
cd your-app && daedalus build . -o myapp.de
./myapp.de
```

## Overview

daedalus transforms any application directory into a portable, self-extracting binary that can run on target machines without requiring the host runtime. This includes:

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

The `.de` binary is structured as:

```
[stub][payload][metadata][footer]
```

- **Stub**: statically-linked launcher — Mach-O on macOS, PE on Windows, musl ELF on Linux — that reads its own binary, verifies integrity, extracts the payload, and `execvp`s the entrypoint
- **Payload**: zstd-compressed tar archive (or squashfs) of the application + runtime
- **Metadata**: JSON with runtime info, entrypoint, layers, capabilities, version
- **Footer**: 4-byte magic `0xBEEF_CAFE`, format version, integrity SHA-256 hash

Format versions: v2 (plain), v3 (signed), v4 (encrypted), v5 (squashfs).

## Target & host support

`daedalus build --target <list>` packages one artifact **per target**. A comma-separated
list emits one `<name>-<target>.de` per target into the output directory. The
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
| `daedalus build <dir>` | Package an app directory into a `.de` file |
| `daedalus inspect <file>` | Read metadata from a `.de` file |
| `daedalus scan [dir]` | Find `.de` files recursively and display metadata |
| `daedalus sign <file>` | Sign a `.de` with an Ed25519 private key |
| `daedalus verify <file>` | Verify the signature of a `.de` against trusted keys |
| `daedalus keygen` | Generate an Ed25519 keypair for signing |
| `daedalus trust <keyfile>` | Add a public key to the trusted keys directory |
| `daedalus doctor` | Check system prerequisites and report missing dependencies |
| `daedalus upgrade` | Self-update the daedalus binary |
| `daedalus env` | Show daedalus environment and build configuration |
| `daedalus clean` | Remove daedalus cache and build artifacts |
| `daedalus completion <shell>` | Generate shell completions for bash, zsh, fish, elvish, or powershell |
| `daedalus man [dir]` | Generate man pages to the specified directory |

### Key features
- **Global `--verbose`** flag for detailed output on any command
- **`--strict`** mode on `daedalus doctor` for strict validation
- **`--dry-run`** flag on build, inspect, scan for preview operations
- **`.daedalus.toml`** configuration file support
- **Rust-based CLI** with zero Python dependency at runtime
- **Self-updating** via `daedalus upgrade`

### Advanced build options

```bash
# Basic build
daedalus build ./myapp -o myapp.de

# Embed interpreter for self-contained binaries
daedalus build ./myapp --embed-interpreter python3 -o myapp.de

# Multi-arch packaging: one artifact per target
daedalus build ./myapp --target linux-x64,linux-arm64 -o out/app.de
#  -> out/app-linux-x64.de, out/app-linux-arm64.de

# Cross-OS packaging: Windows artifact built on Linux/macOS
daedalus build ./myapp --target win-x64 -o out/app.de
#  -> out/app-win-x64.exe (stubs+runtime selected for the target OS)

# Sign your binary
daedalus keygen --key-dir ~/.daedalus/keys
daedalus build ./myapp --sign --key ~/.daedalus/keys/*.key -o myapp.de

# Self-updating binary with SISR (delta updates)
daedalus build ./myapp --enable-sisr --update-url https://updates.example.com -o myapp.de
#  -> myapp.de + myapp.de.manifest

# Persistent storage
daedalus build ./myapp --persist -o myapp.de
#  -> DAEDALUS_PERSIST_DIR injected into app environment

# Environment injection
daedalus build ./myapp --env-file .env --env KEY=VALUE -o myapp.de
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
daedalus build ./myapp --use-cache

# Clear cache before building
daedalus build ./myapp --clear-cache
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

Place a `.daedalus.toml` in your app directory. CLI flags override config file values.

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
daedalus completion bash >> ~/.bashrc

# Zsh
daedalus completion zsh >> ~/.zshrc

# Fish
daedalus completion fish > ~/.config/fish/completions/daedalus.fish
```

### Man pages

```bash
daedalus man /usr/local/share/man/man1/
```

## Build from source

Requires Rust toolchain and `cargo`.

```bash
git clone https://github.com/Tednoob17/daedalus.git
cd daedalus
cargo build --release
# Binary at target/release/daedalus
```

### Cargo workspace

| Crate | Purpose |
|-------|---------|
| `daedalus-core` | Shared library: format, compression, detection, signing, assembly, CAS, SISR |
| `daedalus-stub` | Self-extracting launcher (Linux ELF / macOS Mach-O / Windows PE) |
| `daedalus-cli` | CLI tool (cross-platform) |

## Security

- **Ed25519 signing**: binaries can be signed and verified against trusted keys (`~/.daedalus/trusted-keys/`, or `$DAEDALUS_TRUSTED_DIR`)
- **SHA-256 integrity**: footer hash verifies payload tampering at runtime
- **Namespace isolation**: Linux builds use user/mount namespaces + optional seccomp (fail-closed); macOS uses App Sandbox (`macos_sandbox`); Windows uses process isolation (`win`/supervisor). `--isolation sandbox` must not silently degrade.
- **Delta updates (Sisir)**: the stub verifies an embedded Ed25519 `SisirFooterExt.signature` (of the delta manifest) at cold start — fail-closed, with `DAEDALUS_SISR_ALLOW_UNSIGNED=1` as an explicit escape hatch for air-gapped/untrusted-update scenarios.
- **CVE-2023-48022**: ed25519-dalek pinned to >= 2.1.0 to ensure Ed25519 bit is set

## Troubleshooting

<!-- TROUBLESHOOTING_START -->

### Common issues

**Build fails with "runtime not found"**
Run `daedalus doctor` to check prerequisites. If a runtime is missing, install it:
```bash
# Python
pip install -r requirements.txt

# Node
npm install

# Ruby
bundle install
```

**Build succeeds but app won't run**
Check isolation mode. If the target system doesn't support user namespaces:
```bash
daedalus build ./myapp --isolation none -o myapp.de
```

**Error: "permission denied" when building**
Ensure the output directory is writable:
```bash
daedalus build ./myapp -o myapp.de
```

**"file not found" when running the binary**
The binary may have been moved. Rebuild with a fresh path:
```bash
daedalus build ./myapp -o myapp.de && chmod +x myapp.de
```

**Debug mode**
Enable verbose output to diagnose issues:
```bash
daedalus build ./myapp -v -o myapp.de
```

<!-- TROUBLESHOOTING_END -->

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

## Community

- [GitHub Issues](https://github.com/Tednoob17/daedalus/issues) — report bugs or request features
- `daedalus feedback --browser` — open the feedback page quickly
- [Contributing Guide](#contributing) — how to contribute code or docs
# daedalus
