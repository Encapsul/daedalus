# Roadmap

> **Product evolution ideas**: See [ROADMAP_IDEAS.md](ROADMAP_IDEAS.md) for
> detailed tiered proposals (Tier 1 CI/CD + Registry + Plugins, Tier 2
> multi-sidecar + DAG + OCI bridge, Tier 3 enterprise, Tier 4 breakthrough).
> This file tracks runtime/CLi/build/security implementation status.

## Runtimes

### Complete / production-ready
- Python (Django, FastAPI, Flask, Streamlit)
- Node.js (Next.js, Express, NestJS, Bun, Fastify, Hono)
- Java (Spring Boot, Maven, Gradle)
- Ruby (Rails, Jekyll, Sinatra)
- PHP (Laravel, FrankenPHP, RoadRunner, WordPress)
- Go (static binary, cross-compile)
- .NET/C# (self-contained, cross-RID)
- Binary (ELF/PE staging)

### Partial — needs work

| Runtime | Gap | Priority |
|---------|-----|----------|
| Deno | No toolchain download, no `deno cache`, no cross-compile | P1 |
| Hugo | No binary embed, no `hugo` build step, no theme install | P1 |
| Rust | No cargo auto-download, workspace virtual manifests | P1 |
| Electron | No electron binary embed, undocumented | P2 |
| Wasm | No wasmtime embed, experimental flag | P2 |
| Perl | No Mojolicious detection | P3 |

### Missing — planned

| Runtime | Rationale |
|---------|-----------|
| Swift | iOS/macOS apps, trending via SwiftUI |
| Kotlin | Android/JVM backend, growing |
| Lua | Game mods, Neovim configs, OpenResty |
| Dart/Flutter | Mobile + web, growing |
| Zig | Trending language, single binary |
| OCaml/Elm | Functional web, niche but real |
| R | Data science, Shiny apps |
| Elixir | Phoenix framework, real-time |
| Crystal | Ruby-like, compiled |
| Nim | Python-like, compiled |
| D | Systems programming |
| V | Trending, simple syntax |
| MoonBit | New, WASM-targeted |
| Gleam | Typed BEAM, growing |

## CLI features

| Feature | Status |
|---------|--------|
| Global `--plain` | Done |
| Global `--no-input` | Done |
| Global `--json` | Partial (per-command) |
| Pager support | Done |
| Typo suggestions | Done |
| `-` stdin/stdout | Not started |
| Shell completions | Done |
| Man pages | Done |
| `daedalus run` | Done |
| `daedalus inspect --plain` | Done |
| `daedalus scan --json` | Done |
| `daedalus registry --json` | Done |

## Build pipeline

| Feature | Status |
|---------|--------|
| Cross-compile stubs | Partial (Linux ELF done, macOS/Windows TODO) |
| SISR delta updates | Done |
| Encryption (AES-256-GCM) | Done |
| Ed25519 signing | Done |
| Squashfs payload | Done |
| jlink minimal JRE | Not started |
| Build cache | Done |
| Parallel multi-target | Done |

## Security

| Feature | Status |
|---------|--------|
| Seccomp filter | Done |
| User/mount namespaces | Done |
| macOS App Sandbox | Done |
| Windows process isolation | Done |
| Ed25519 bit validation (CVE-2023-48022) | Done |
| SISR publisher signature | Done |
| At-rest authenticity | Roadmap #45 |
