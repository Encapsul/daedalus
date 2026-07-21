# x.bin

[![Build Status](https://img.shields.io/github/actions/workflow/status/Tednoob17/x.bin/release.yml?branch=main)](https://github.com/Tednoob17/x.bin/actions)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.3.0-green.svg)](https://github.com/Tednoob17/x.bin/releases)

Package any app into a single self-extracting binary.

x.bin compiles web, server, and CLI applications -- with their runtime, dependencies, and data -- into a single self-contained ELF executable. Auto-detects your runtime and framework, bundles everything with zstd or SquashFS compression, and signs the result with Ed25519. Built in Rust with no Python dependency required at runtime.

## Quick start

```bash
curl -fsSL https://raw.githubusercontent.com/Tednoob17/x.bin/main/scripts/install.sh | bash
xbin doctor
cd your-app && xbin build . -o myapp.xbin
./myapp.xbin
```

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

## Configuration

Place a `.xbin.toml` in your app directory. CLI flags always override config values.

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
  --tree-shake \
  --minify \
  --update \
  --dry-run \
  --verbose
```

## Shell completion

```bash
# Bash
xbin completion bash >> ~/.bashrc

# Zsh
xbin completion zsh >> ~/.zshrc

# Fish
xbin completion fish > ~/.config/fish/completions/xbin.fish
```

## Man pages

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
cargo clippy -- -D warnings
cargo test
cargo fmt --check
```

## License

MIT -- see [LICENSE](LICENSE) for details.
