# Roadmap

Single source of truth for daedalus planning. `docs/ROADMAP.md` points here;
`docs/src/roadmap.md` is the public summary for the website.

## Current status

Cross-platform CI is green: Linux, macOS native (all steps incl. smoke),
Windows (`windows-check` and `smoke-test-windows`), the four cross-builds
and cargo-audit all pass. The previously blocking Windows failures were the
Rust 1.99 lint drift, fixed in `b21fa84`. The smoke app is
`examples/hello-web` (Python stdlib). The offline-clinic tree is a scaffold
with no model weights. Linux CI now also smokes `examples/bottle-web` (vendored deps) and
`examples/field-kit` (adjacent-domain modes). Do not add runtimes or
commands until those smokes stay boring.

## Runtimes

### Detection is implemented

These runtimes have a detector and a build path in tree. That is not a CI
end-to-end of the framework named in parentheses. Linux CI smokes Python
stdlib (`hello-web`), vendored deps (`bottle-web`), and adjacent modes
(`field-kit`). Rust has a workspace e2e when cargo can build a stub.
Java/Gradle e2e skips unless those tools are installed.

- Python (Django, FastAPI, Flask, Streamlit markers — CI smoke is stdlib HTTP)
- Node.js (Next.js, Express, NestJS, Bun, Fastify, Hono) — example in repo, not CI
- Java (Spring Boot, Maven, Gradle) — e2e skips without java + Gradle ≥ 7
- Ruby (Rails, Jekyll, Sinatra)
- PHP (Laravel, FrankenPHP, RoadRunner, WordPress)
- Go (static binary, cross-compile)
- Rust (cargo build) — workspace e2e
- .NET/C# (self-contained, cross-RID)
- Binary (ELF/PE staging)
- Deno (toolchain download + deno cache)
- Hugo (binary embed + hugo build step)
- Wasm (wasmtime embed)
- Ollama (detection + `ollama serve` entrypoint). Weights only if present on disk
- Gemma (detection from a Gemma `Modelfile` or `models/gemma*.gguf`). The
  in-repo example ships neither a GGUF nor Ollama
- Zig (example in repo)
- Dart (example in repo; host-only AOT)
- Flutter (requires the Flutter SDK on PATH; no SDK download)
- Perl (Mojolicious-specific detection)
- Electron (cross-OS/arch binary embed)
- Lua (example in repo)

### Frozen

Linux CI now smokes `hello-web`, `bottle-web` (vendored deps), and `field-kit`
(adjacent modes). Still no new runtime until those stay green for a full week.
Swift, Kotlin, OCaml, Elm, R, Elixir remain out of scope.

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
| Air-gap media stage (`daedalus media stage` → USB manifest+chunks) | Done |
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
| Capability-based syscall filter (seccomp denylist + optional Landlock) | Done. Denylist of ~18 syscalls, not an app profile |
| At-rest authenticity (SISR manifest + Ed25519 checked at cold start) | Done |

## Product & adoption

North star: one Linux ELF a machine without the runtime can execute
(`./app.daedalus`). The filename alias `.de` is accepted by scan, publish,
and registry.

| Lever | Action | Status |
|-------|--------|--------|
| Demo / time-to-first-value | 60 s homepage demo: `daedalus build examples/hello-web` → an ELF that serves HTTP. The clinic tree is a scaffold, not the demo. | Done for hello-web. Clinic numbers (6.2 s / 16.9 MB / Gemma) withdrawn |
| Trust (#1) | `daedalus trust <pubkey>` then `daedalus verify` on a second machine. Default dev-key auto-trust only proves the file to the builder. | Default signing done. Recipient flow documented in the README. Public security page is `docs/src/security.md` |
| Niche wedge | Agents / local models for non-technical users. | Started: `examples/local-model-kit` + `fetch_weights.sh` (Gemma 3 270M Q4 from HF, gitignored). Pitch only when GGUF is present at build |
| Ecosystem / network effect | `hub/catalog.json`: examples that build, plus recipes that do not. | Started. Four-plus in-repo apps build. Recipes are not verified runs |
| Zero-friction install | `brew` / `cargo install daedalux` / `pip install daedalux` / root `install.sh` (sha256 required, no sudo). | Four channels. `scripts/install.sh` is the older copy and now also refuses a missing checksum |
| Trap to avoid | No expert-oriented docs or format benchmarks as the lead feature - adoption comes from the first task unlocked. | - |

Execution order: demo (1) → default signing (2) → minimal hub with ~10 packaged apps
(4) → AI-app word of mouth (3).
