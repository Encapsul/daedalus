# Roadmap

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
- Deno (toolchain download + deno cache)
- Hugo (binary embed + hugo build step)
- Wasm (wasmtime embed)

### Partial — needs work

| Runtime | Gap | Priority |
|---------|-----|----------|
| Rust | No cargo auto-download, requires cargo on PATH | P1 |
| Electron | Detection done, no cross-compiled binary embed (falls back to host) | P2 |
| Perl | Detection done, no Mojolicious-specific detection | P3 |

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
| Cross-compile stubs | Done (`daedalus build --universal`, polyglot shell launcher) |
| SISR delta updates | Done |
| Encryption (AES-256-GCM) | Done |
| Ed25519 signing | Done |
| Squashfs payload | Done |
| jlink minimal JRE | Not started |
| Build cache | Done |
| Parallel multi-target | Done |
| Universal binary (`--universal`) | Done (polyglot shell launcher, multi-arch slices) |
| Hot-swap layers (`daedalus swap`) | Done |
| Registry CAS (`daedalus registry push/pull/list`) | Done |

## Security

| Feature | Status |
|---------|--------|
| Seccomp filter | Done |
| User/mount namespaces | Done |
| macOS App Sandbox | Done |
| Windows process isolation | Done |
| Ed25519 bit validation (CVE-2023-48022) | Done |
| SISR publisher signature | Done |
| Capability-based sandboxing (seccomp + Landlock) | Done |
| At-rest authenticity | Roadmap #45 |
