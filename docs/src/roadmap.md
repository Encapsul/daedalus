# Roadmap

## Phase 1 — MVP functional ✅

- [x] `.ere` format defined and stable (versioned 84B footer)
- [x] Rust/musl static launcher: self-read, SHA-256 integrity, exec
- [x] Atomic cache extraction using `rename()`
- [x] Python builder: runtime detection, rootfs construction, assembly
- [x] CLI: `build`, `run`, `inspect`, `clean`
- [x] Python support (stdlib) — `hello-web` example
- [x] `flock()` for concurrent cache access
- [x] `erebus clean`
- [x] Python `site-packages` / `.venv` support — `bottle-web` example
- [x] **v2 layered format + incremental rebuild** (runtime reused, rebuild ~25s → ~1s; shared build cache between apps)
- [x] `requirements.txt` → pip install at build time (temp venv)
- [x] Node.js end-to-end support (stdlib + node_modules)
- [x] Deno support (deno.json detection, `--allow-all`, tasks-based entrypoint)

## Phase 2 — Robustness ✅

- [x] **Pure-Python ELF analyzer** (no host `ldd` dependency, works on any machine with Python)
- [x] **Ed25519 signatures** (v3 footer, `erebus keygen` / `sign` / `verify` / `trust`)
- [x] **Trust model**: `$XDG_DATA_HOME/erebus/trusted-keys/` keyring, `erebus trust` subcommand
- [x] **Level 2 isolation**: user namespaces + `pivot_root` (real portability)
- [x] **Smart `.so` deduplication** (removes duplicate `ld-linux` instances)
- [x] `/etc/hosts` in rootfs (prevents DNS PTR lookup hang)
- [x] **Self-hosting**: `self/` → `erebus build self/` → `./erebus build ...` (full cycle: CLI → `erebus-v2` → `erebus-v3` → `erebus-v3`)
- [x] **Dockerfile dependency detection** (apt/apk/pip/npm packages + external binary fetch chains)
- [x] **Python AST scanner** (subprocess/os.system call detection)
- [x] **Dependency fetcher** (isolated staging, non-invasive)
- [x] **PATH injection** (bundled binaries found at runtime)
- [x] **Minimal seccomp filter** (denylist of 16 dangerous syscalls, installed after pivot_root, arch-aware for x86_64 and aarch64)
- [x] **Payload encryption** (AES-256-GCM, v4 format, HKDF key derivation from signing seed, decrypt after sig+integrity verification)
- [ ] Manifest mode (`erebus.toml`) for complex dependencies
- [ ] LRU cache cleanup (evict beyond threshold)

## Phase 3 — Production ✅

- [x] **SquashFS extraction** (v5 format, `--squashfs` flag, `mksquashfs` build, `backhand` Rust parser, better compression than zstd+tar)
- [x] **11 supported runtimes**: Python, Node.js, Deno, Java, Ruby, .NET/C#, Go, PHP, Perl, Binary, Hugo
- [x] **Framework auto-detection**: Next.js, Nuxt, Astro, Remix, SvelteKit, Express, Fastify, Hono, Django, FastAPI, Flask, Laravel, Symfony
- [x] **`.env` file baking** (`--env-file` flag, secret detection)
- [x] **Version metadata** (`--version-info`, `--author`, `--description`, `--license`)
- [x] **Persistent storage** (`--persist` flag, `EREBUS_PERSIST_DIR` env var)
- [x] **Data files** (`--include PATH` flag, repeatable)
- [x] **Tree-shaking** (`--tree-shake`, removes unused node_modules)
- [x] **Minification** (`--minify`, JS/TS via terser, CSS built-in)
- [x] **Health checks** (`--health-port`, `/healthz`, `/readyz`, `/status`)
- [x] **OpenTelemetry** (`--otel-endpoint`, OTLP export, auto-instrumentation)
- [x] **Cron/scheduled tasks** (`--cron NAME:SCHEDULE`, background scheduler)
- [ ] **squashfs + mmap**: direct read, no extraction (kernel mount, Linux 5.12+)
- [ ] **Cold/warm start < 100 ms** end-to-end
- [ ] Distribution / discovery (lightweight registry, even P2P)

## Phase 4 — MLOps demo with PleIAs model (PRIORITY #1)

**Why Phase 4 first** : This phase proves erebus's value with a real, small AI model. Everything in Phase 5+ builds on this.

### Target app: `Pleias-SLM-RAG` (PleIAs)
A 300M-param RAG server (Flask + LanceDB + GGUF model + llama.cpp), 328 MB model file, runs CPU-only on Raspberry Pi 5. Requirements:
- `flask` — web framework (Python, ✓ already supported)
- `lancedb` — vector store (Python, pip-installable)
- `pandas` — data (pip)
- `llama-cpp-python` — llama.cpp Python bindings (pip package with bundled .so)
- `Pleias-RAG.gguf` — 328 MB model weights (large file → app layer)
- `llama-cli` or `llama-server` — native binary (✓ "Binary" runtime already supported)

```
my-rag/
  requirements.txt     ← flask, lancedb, pandas, llama-cpp-python
  app.py              ← Flask API
  Pleias-RAG.gguf     ← 328 MB model weights
  bin/
    llama-server      ← native ELF binary (✓ Binary runtime)
```

### What works TODAY
- [x] Python runtime detection (`Pleias-SLM-RAG` style Flask app)
- [x] `pip install` from requirements.txt (`--embed-interpreter python3`)
- [x] Native binary packaging (llama.cpp ELF binary → "Binary" runtime)
- [x] Large file embedding (328 MB GGUF → app layer + squashfs compression)
- [x] `.so` resolution for `llama-cpp-python` (Rust ELF analyzer reads `DT_NEEDED`)
- [x] PATH injection (llama-server found in rootfs `bin/`)
- [x] SISR incremental updates (existing engine — chunk-based rebuilds)
- [x] Content-addressed build cache (runtime layer reused, app layer rebuilt)

### What's needed for the MLOps demo
- [ ] **`--embed-model`** flag — explicit marker for large model files; tunes FastCDC chunk size for GB files (larger chunks, fewer manifest entries) and enables sparse Merkle verification (see [delta-manifest-format.md](spec/delta-manifest-format.md))
- [ ] **Large-file chunking** — FastCDC tuned for model weights (chunk_target_size = 1MB default → 16MB for `--embed-model`); chunk boundaries stay stable across model quantization (Q4 → Q8 diff only touches boundary chunks)
- [ ] **Model signature + provenance** — Ed25519 over full payload tree; SBOM-style model card embedded in metadata JSON (model name, source hash, quantization level)
- [ ] **Bandwidth reporting** — build report shows SISR savings: `"12 MB delta vs 328 MB full — 96.4%"`
- [ ] **PleIAs test case** — `examples/pleias-rag/` with the Pleias-SLM-RAG app; `erebus build ./examples/pleias-rag --embed-model Pleias-RAG.gguf --to app --enable-sisr`

### Priority ordering rationale
1. `--embed-model` (large-file chunking) — this is the core SISR optimization for AI models
2. Bandwidth reporting — user-facing metric that proves value ("99% smaller updates")
3. Model provenance — needed for production MLOps (auditing, compliance)
4. PleIAs demo — validate end-to-end with real model

## Phase 5 — Multi-format output (`--to` family) (PRIORITY #2)

Building on Phase 3's stable `.erebus` format, this phase makes erebus a **universal packager**: one build command can emit multiple deployment formats while preserving SISR incremental updates across all of them.

### Priority 1: OCI (highest value — container ecosystem dominates)
- [ ] **`--to oci`** — emit an OCI image (tarball or registry push) alongside the `.erebus` file; stub → entrypoint shim; payload → OCI layer with zstd:chunked for lazy pulls
- [ ] **OCI registry backend** — `erebus push registry/app:v1` works with GHCR, Docker Hub, ECR; pull with standard `docker pull`
- [ ] **`--to oci,wasm,app`** — single build → all formats simultaneously

### Priority 2: WASM (edge/browser/serverless)
- [ ] **`--to wasm`** — emit wasm32-wasip2 module; runtime interpreter → WASM via Pyodide/Node-WASM; payload → WASI preopen directories
- [ ] **WASM component model** — optional `--to wasm-component` using `cargo-component` for language-interoperable modules

### Priority 3: AppImage + cross-format SISR
- [ ] **`--to appimage`** — emit self-extracting AppImage; musl stub → AppImage runtime; payload → AppDir squashfs
- [ ] **SISR cross-format** — same FastCDC chunk engine + Merkle-delta manifest serves ALL output formats; single signature chain validates deltas spanning OCI, AppImage, WASM, and `.erebus`
- [ ] **Format-aware cache** — cache keys include output format tag; cached runtime layer reused across `.erebus`, OCI, AppImage, WASM without rebuilding
- [ ] **AppImage `zsync` feed** — backward-compatible incremental updates for AppImage native consumers (fallback when SISR engine not embedded)

### Business model alignment
| Phase | OSS tool | SaaS |
|---|---|---|
| Phase 4 (MLOps demo) | `--embed-model`, bandwidth reporting | Registry for model artifacts + delta hosting |
| Phase 5 (--to family) | `--to oci/appimage/wasm` | Registry push/pull + cross-format SISR hosting |
| | | `$0.05/build` or `$29-dev $149-team $1999-enterprise` |

## Phase 6 — Edge/IoT OTA (planned)

- [ ] **Signed OTA channel** — remote manifest (`<name>.erebus.manifest`) served over HTTPS with Ed25519 signature chain; target device self-updates via `./app.erebus update`
- [ ] **Anti-rollback enforcement** — monotonic version index checked before any delta is applied (see [delta-manifest-format.md](spec/delta-manifest-format.md))
- [ ] **Atomic swap** — staging directory + `rename()` ensures interruption leaves previous version intact
- [ ] **Fleet awareness** — `erebus push --target fleet` writes a delta compatible with all device formats
