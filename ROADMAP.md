# Roadmap

Single source of truth for daedalus planning. `docs/ROADMAP.md` points here;
`docs/src/roadmap.md` is the public summary for the website.

## Current status

Cross-platform CI is the open item: Linux CI is green; macOS native (all steps
incl. smoke) and cargo-audit are green since `60625dd`. Remaining: the two
Windows jobs (`windows-check` core test, `smoke-test-windows`) — pre-existing
failures blocked on CI log access (read-only token). Product-wise: adoption
is underway — 60 s demo guide (measured 6.2 s build / 1.5 s run), one-command
installers (`install.sh`, `install.ps1`, Homebrew formula), `hub/catalog.json`
(4 verified apps + 6 recipes); runtimes are production-grade on Linux and Java
JRE embed (jlink) works on macOS.

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
- Rust (cargo build, auto-download toolchain; auto-downloads `rustup-init` + installs stable into `~/.cache` when no system cargo/rustup)
- .NET/C# (self-contained, cross-RID)
- Binary (ELF/PE staging)
- Deno (toolchain download + deno cache)
- Hugo (binary embed + hugo build step)
- Wasm (wasmtime embed)
- Ollama (detection + `ollama serve` entrypoint)
- Gemma (offline `.gguf` bundling via `--model`, `ollama run <model>`, no cloud/GPU)
- Perl (Mojolicious-specific detection: script/ + lib/ layout, `Mojo::` imports)
- Electron (cross-OS/arch binary embed, OS-aware `is_cross`, `resources/` embedded beside binary)

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
| jlink minimal JRE | Done |
| Build cache | Done |
| Parallel multi-target | Done |
| Universal binary (`--universal`) | Done (polyglot shell launcher, multi-arch slices) |
| Hot-swap layers (`daedalus swap`) | Done |
| Registry CAS (`daedalus registry push/pull/list`) | Done |
| Lazy loading (`--lazy-load`) | Done (priority extraction + background thread) |
| Multi-service build (`--entrypoint service=cmd`) | Done |
| Metadata templates (`--template application|service|plugin`) | Done |

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
| At-rest authenticity (SISR manifest + Ed25519 checked at cold start) | Done |

## Product & adoption

North star: make any app **consumable in one gesture** (`./app.de`) — no install, no
expertise. Packaging is the decisive adoption factor (VLC lesson: the user sees the movie,
not the codecs).

| Lever | Action | Status |
|-------|--------|--------|
| Demo / time-to-first-value | 60 s homepage demo (Streamlit or Ollama + model): `daedalus build` → an artifact that runs on a bare machine. Key message: "it's just the file." | Done (measured: 6.2 s build / 1.5 s first run for the offline clinic-agent; guide in `docs/src/guides/demo-60s.md`) |
| Trust (#1) | `--sign` on by default with the dev key, `daedalus verify foo.de` in one gesture, dated "security" page + audit. Signing is the headline feature, not an option (a self-extracting binary smells like malware otherwise). | Default signing done (auto dev key + self-trust, `--skip-sign` to opt out); dated audit 2026-09-09 in SECURITY.md (strict Ed25519 verify); website security page remaining |
| Niche wedge | Target distribution of agents / AI-apps to non-technical users (Ollama/Gemma use cases). A niche of 1000 frustrated devs > 100k curious. | Not started |
| Ecosystem / network effect | `daedalus hub` — community catalog of reusable packaged apps (builds on `daedalus registry`). Start with ONE template per popular runtime, not a platform. | Started (`hub/catalog.json`: 4 verified apps + 6 recipes — one per runtime; `hub/build.sh`; guide in `docs/src/guides/hub.md`) |
| Zero-friction install | `brew` / `cargo install` / `pip` / `curl` install, static signed binary every release. Install < 10 s, no compile flag needed. | `install.sh` (Linux/macOS) + `install.ps1` (Windows) shipped (checksum-verified, no sudo); cargo install OK; brew tap + crates.io + pip remaining |
| Trap to avoid | No expert-oriented docs or format benchmarks as the lead feature — adoption comes from the first task unlocked. | — |

Execution order: demo (1) → default signing (2) → minimal hub with ~10 packaged apps
(4) → AI-app word of mouth (3).
