<div align="center">

# x.bin &nbsp;·&nbsp; `xbin`

**Ship your web app like a binary. Run anywhere.**

[![Status](https://img.shields.io/badge/status-MVP%20functional-brightgreen)](/docs/src/roadmap.md)
[![Python](https://img.shields.io/badge/python-%3E%3D3.10-3670A0?logo=python&logoColor=ffdd54)](/cli)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-000000?logo=rust&logoColor=white)](/stub)
[![License](https://img.shields.io/badge/license-MIT-blue)](/LICENSE)
[![Contributing](https://img.shields.io/badge/PRs-welcome-brightgreen)](/CONTRIBUTING.md)
[![Built for](https://img.shields.io/badge/built%20for-YC-green)](#)

```bash
chmod +x myapp.xbin && ./myapp.xbin
# Server listening on http://127.0.0.1:8080
```

Zero runtime to install. Zero Docker. One file that runs everywhere.

</div>

---

## The Problem

Distributing a server app is broken. Every deployment is a gamble:

- Wrong Python/Node version on the target machine
- Missing `.so` shared libraries
- Different distro, different paths
- Docker drags in a daemon, root access, and a whole ceremony

The result is the oldest problem in software: *"it works on my machine."*

## The Solution

`xbin` takes the opposite approach: it packages your app **and everything it needs** (runtime, shared libraries, packages, config) into a single self-extracting ELF executable. The user downloads one file, `chmod +x` it, and runs it. That's it.

```
┌──────────────────────────────────────────────────┐
│  xbin build ./my_app → produces my_app.xbin      │
│                                                   │
│  my_app.xbin:  [ELF launcher][payload][meta][ftr] │
│                ├─ Rust/musl static (~600KB)       │
│                ├─ zstd(tar) rootfs with runtime   │
│                ├─ JSON metadata + SHA-256 footer  │
│                └─ 84-byte versioned footer        │
└──────────────────────────────────────────────────┘
```

### Key differentiators

| vs | xbin wins on |
|---|---|
| **AppImage** | targets **web/server headless**, not desktop GUI |
| **Docker** | **no daemon, no root, one file** — not an orchestrator |
| **pkg/PyInstaller** | **language-agnostic** — Python, Node, Go, native binary |
| **Go static binary** | same UX for **scripted runtimes** (Python, Node, etc.) |

## Quick start

```bash
# Prerequisites: Linux x86_64, Rust musl target, Python ≥3.10, zstd
make stub           # compile the Rust launcher (musl static)
make example        # build examples/hello-web → ./hello-web.xbin (7 MB)
./hello-web.xbin    # starts an HTTP server, zero host dependencies
```

Visit `http://127.0.0.1:8080` in your browser.

```bash
xbin inspect hello-web.xbin
# name:            hello-web
# runtime:         python
# entrypoint:      /usr/bin/python3.12 /app/app.py
# payload:         6.4MB compressed / 26.4MB raw
```

## How it works

```
MACHINE DE DEV                        MACHINE CIBLE

my_app/         xbin build    my_app.xbin      ./my_app.xbin    ça tourne
  app.py       ───────────→  (1 file)        ─────────────→
  .venv/                     ┌──────────┐                    ┌──────────┐
  requirements.txt           │xbin-stub │  /proc/self/exe     │  cache   │
+ python3                    │payload ⊳─┼──────────────────→  │ + execve │
+ libs .so                   │footer    │                    └──────────┘
                             └──────────┘
```

**At build time**, `xbin build`:
1. Detects the runtime (Python, Node, or native binary)
2. Resolves shared libraries via a pure-Python ELF parser (no host `ldd` needed)
3. Packages interpreter + stdlib + `.so` into a **runtime layer**
4. Packages app code + site-packages into an **app layer**
5. Compresses each layer with `zstd -19`, assembles the `.xbin`

Both layers are `tar`'ed deterministically (normalized mtime/uid/gid, sorted entries) so identical content produces identical bytes — enabling the build cache.

**At runtime**, the launcher:
1. Opens `/proc/self/exe` (not `argv[0]` — more reliable)
2. Reads the 84-byte footer at end-of-file, validates magic
3. Reads metadata JSON, verifies SHA-256 integrity
4. Checks `~/.cache/xbin/{hash}/` — extracts if missing (atomic `rename()`)
5. Builds argv/env with `LD_LIBRARY_PATH` pointing into the extracted rootfs
6. `execve()` — replaces itself with the embedded app

### Layered format (v2) + incremental rebuild

The format splits the payload into independent **layers**, similar to Docker:

```
[ stub ][ runtime layer (stable) ][ app layer (volatile) ][ metadata ][ footer ]
```

- The **runtime layer** (interpreter + stdlib + `.so`) rarely changes — it's cached in `~/.cache/xbin/build/{hash}.zst`
- The **app layer** (code + site-packages) is small and rebuilt every time

```
Initial build : ~25 s  (compressing runtime layer, ~26 MB)
Rebuild (code): ~1 s   (runtime layer reused from build cache — no recompression)
```

Two apps sharing the same runtime share the build cache entry. See the [format spec](/docs/src/reference/format.md).

## Architecture

```
CLI (xbin)            build · run · inspect · clean
   │
Builder               Analyzer (ldd + runtime) · Packager (rootfs + zstd)
   │
Format .xbin          [ ELF launcher ][ payload zstd ][ metadata JSON ][ footer ]
   │
Runtime (launcher)    /proc/self/exe · atomic cache · execve

Four decoupled layers, joined by the .xbin format — the shared contract.
```

See the full [architecture docs](/docs/src/concepts/architecture.md) (French) for details.

## CLI reference

```bash
xbin build ./my_app -o my_app.xbin            # analyze + produce .xbin
xbin build ./my_app --isolation 2             # with user namespaces + pivot_root
xbin run   my_app.xbin                        # launch (= ./my_app.xbin)
xbin inspect my_app.xbin                      # show contents without extracting
xbin clean                                    # remove extracted cache entries
xbin clean --all                              # wipe everything (including build cache)
```

Debug: `XBIN_VERBOSE=1 ./my_app.xbin` shows cold/warm start info.

## Example apps

| Example | What it demonstrates |
|---|---|
| [`hello-web`](/examples/hello-web) | Python stdlib HTTP server — zero dependencies |
| [`bottle-web`](/examples/bottle-web) | Python app with a third-party dependency (bottle) from `.venv` |

Build them yourself:
```bash
make example          # hello-web
xbin build ./examples/bottle-web -o bottle-web.xbin
```

## Status

**MVP functional** — Phase 1. The full pipeline works end-to-end.

### ✅ Done
- Format `.xbin` versioned (v1 → v2 layered), 84-byte footer
- Rust/musl static launcher: self-read, SHA-256 integrity, `execve`
- Atomic cache extraction (`rename()` + `flock()`)
- Python: stdlib + site-packages (`.venv` / `site-packages/`)
- Incremental rebuild (runtime layer cached, ~25s → ~1s)
- CLI: `build`, `run`, `inspect`, `clean`

### ✅ Phase 2 — Robustness
- Ed25519 signatures (`xbin keygen` / `sign` / `verify`)
- Trust model (`~/.xbin/trusted-keys/` keyring)
- Isolation level 2 (user namespaces + `pivot_root`)
- Pure-Python ELF parser (no host `ldd` dependency)
- `requirements.txt` → auto pip-install at build time (temp venv)
- Node.js end-to-end support
- Self-hosting: `self/` → `xbin build self/` → `./xbin build ...`

### 🔜 Phase 3 — Production
- AI dependency analyzer (detects `subprocess`, `dlopen`, hidden deps)
- Manifest mode (`xbin.toml`)
- squashfs + mmap (cold start < 100ms)

### 🔜 Phase 3 — Production
- squashfs + mmap (cold start < 100ms)
- Cross-arch (aarch64)
- Multi-runtime (Ruby, Java/GraalVM, Deno)

See the [full roadmap](/docs/src/roadmap.md) (French).

## Repository structure

```
xbin/
├── stub/              Rust launcher (musl static)
│   └── src/
│       ├── main.rs    self-read → verify → cache → exec
│       └── format.rs  footer parser (sync'd with Python)
├── cli/               Python CLI + builder (stdlib only)
│   └── xbin/
│       ├── cli.py     build / run / inspect / clean
│       ├── build.py   rootfs construction + assembly
│       ├── format.py  footer writer (sync'd with Rust)
│       ├── inspect.py
│       └── analyzer/
│           ├── elf.py       pure-Python ELF parser (replaces host `ldd`)
│           ├── ldd.py       thin facade calling `elf.py`
│           └── runtime.py   runtime detection + entrypoint
├── examples/          demo apps
├── docs/              mdbook documentation (French)
└── Makefile
```

## Documentation

Full documentation (concepts, reference, guides, roadmap) is available as an mdbook:

```bash
make docs          # build → docs/book/
make docs-serve    # serve on http://localhost:3000 with live-reload
```

The documentation is in **French** (the builder's native language). Code comments and this README are in English.

## Why now?

Three converging trends make `xbin` timely:

1. **Local AI is exploding** — distributing `llama.cpp` + a model + a web UI + an inference server as a single file has no clean solution today. Docker is overkill, AppImage is desktop-only, PyInstaller can't handle native C binaries.
2. **Node 21+ has built-in SEA** but it's Node-only. The industry needs a **language-agnostic** equivalent.
3. **Rootless containers are mainstream** — user namespaces, available since Linux 3.8, finally make real filesystem isolation possible without privileges. That unlocks what `xbin` needs for cross-distro portability.

## Why this team?

<!-- TODO: Add your background. For YC, focus on: domain expertise, 
     past open-source work, and why you're the right person to build this. -->

---

## License

MIT — see [LICENSE](LICENSE).

## Security

See [SECURITY.md](SECURITY.md) for the security policy and current posture.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines. PRs welcome!
