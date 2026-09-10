# Roadmap

## Runtimes

### Complete / production-ready
- Python (Django, FastAPI, Flask, Streamlit)
- Node.js (Next.js, Express, NestJS, Bun, Fastify, Hono)
- Java (Spring Boot, Maven, Gradle)
- Ruby (Rails, Jekyll, Sinatra)
- PHP (Laravel, FrankenPHP, RoadRunner, WordPress)
- Perl (Mojolicious, single-file `Mojo::` apps)
- Electron (cross-arch/OS binary embed, `resources/` bundled)
- Go (static binary, cross-compile)
- Rust (cargo build, auto-download toolchain)
- .NET/C# (self-contained, cross-RID)
- Binary (ELF/PE staging)
- Deno (toolchain download + deno cache)
- Hugo (binary embed + hugo build step)
- Wasm (wasmtime embed)
- Ollama (detection + `ollama serve` entrypoint)
- Gemma (offline `.gguf` bundling via `--model`, `ollama run <model>`, no cloud/GPU)

### Partial — needs work

| Runtime | Gap | Priority |
|---------|-----|----------|
| Rust | ✅ Auto-downloads `rustup-init` + installs stable into `~/.cache` when no system cargo/rustup | ~~P1~~ |
| Electron | ✅ Cross-OS/arch binary embed (OS-aware `is_cross`, `resources/` embedded beside binary) | ~~P2~~ |
| Perl | ✅ Mojolicious-specific detection added (script/ + lib/<App>.pm layout, `Mojo::` imports) | ~~P3~~ |

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
| Global `--json` | Done (all subcommands) |
| Pager support | Done |
| Typo suggestions | Done |
| `-` stdin/stdout | Done (`build -o -`, `run -`, `inspect -`, `sign -`, `verify -`, `swap -`) |
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
| Lazy loading (`--lazy-load`) | Done (priority extraction + background thread) |
| Multi-service build (`--entrypoint service=cmd`) | Done |

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

## Product & adoption

North star: make any app **consumable in one gesture** (`./app.de`) — no install, no
expertise. Packaging is the decisive adoption factor (VLC lesson: the user sees the movie,
not the codecs).

| Lever | Action | Status |
|-------|--------|--------|
| Demo / time-to-first-value | 60 s homepage demo (Streamlit or Ollama + model): `daedalus build` → an artifact that runs on a bare machine. Key message: "it's just the file." | Not started |
| Trust (#1) | `--sign` on by default with the dev key, `daedalus verify foo.de` in one gesture, dated "security" page + audit. Signing is the headline feature, not an option (a self-extracting binary smells like malware otherwise). | Default signing done (auto dev key + self-trust, `--skip-sign` to opt out); dated audit 2026-09-09 in SECURITY.md (strict Ed25519 verify); website security page remaining |
| Niche wedge | Target distribution of agents / AI-apps to non-technical users (Ollama/Gemma use cases). A niche of 1000 frustrated devs > 100k curious. | Not started |
| Ecosystem / network effect | `daedalus hub` — community catalog of reusable packaged apps (builds on `daedalus registry`). Start with ONE template per popular runtime, not a platform. | Not started |
| Zero-friction install | `brew` / `cargo install` / `pip` / `curl` install, static signed binary every release. Install < 10 s, no compile flag needed. | cargo install OK; others to do |
| Trap to avoid | No expert-oriented docs or format benchmarks as the lead feature — adoption comes from the first task unlocked. | — |

Execution order: demo (1) → default signing (2) → minimal hub with ~10 packaged apps
(4) → AI-app word of mouth (3).
