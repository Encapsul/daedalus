<div align="center">

# x.bin &nbsp;·&nbsp; `xbin`

**Ship your app as one file.**

[![License](https://img.shields.io/badge/license-MIT-blue)](/LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-000000?logo=rust&logoColor=white)](/stub)
[![Python](https://img.shields.io/badge/python-%3E%3D3.10-3670A0?logo=python&logoColor=ffdd54)](/cli)
[![Status](https://img.shields.io/badge/status-MVP%20functional-brightgreen)](/docs/src/roadmap.md)
[![Contributing](https://img.shields.io/badge/PRs-welcome-brightgreen)](/CONTRIBUTING.md)

</div>

```bash
xbin build ./my-app -o my-app.xbin
chmod +x my-app.xbin && ./my-app.xbin
# → serving on :8080
```

No runtime to install. No Docker. No dependency resolution on the target
machine — the file just runs.

---

## What it is

`xbin` packages a Python or Node.js web/server/CLI app — code, runtime,
shared libraries, and metadata — into a **single self-extracting ELF
executable**. The launcher (~615KB, Rust, static musl) reads itself,
verifies integrity, extracts to a local cache, and execs the app. One file,
any compatible Linux machine.

## See it work

A real terminal session — build a Python app, run it, inspect it, build a
Node.js app with the same CLI, do an incremental rebuild, then sign and
verify:

```
$ xbin build examples/hello-web -o hello-web.xbin
[xbin] building 'hello-web'
  runtime: python
  runtime layer: reused from build cache (no recompression) ✓
[xbin] wrote hello-web.xbin (7.1MB, unsigned) in 0.6s

$ ./hello-web.xbin
Server listening on http://127.0.0.1:8080

$ xbin build examples/hello-node -o hello-node.xbin      # same CLI, different runtime
[xbin] building 'hello-node'
  runtime: node
[xbin] wrote hello-node.xbin (27.5MB, unsigned) in 0.8s

$ xbin keygen --key-dir ~/.xbin/keys -q
bf68e4e5471d...

$ xbin sign hello-web.xbin --key ~/.xbin/keys/bf68e4e5.key
[xbin] signed hello-web.xbin

$ xbin verify hello-web.xbin --trusted-dir ~/.xbin/trusted
[xbin] signature verified for hello-web.xbin
```

*(Full recording: [`demo.cast`](demo.cast) — play with
[asciinema](https://asciinema.org): `asciinema play demo.cast`)*

## The problem

Distributing a server app is a gamble every time:

- Wrong Python/Node version on the target machine
- Missing `.so` shared libraries
- Different distro, different paths
- Docker drags in a daemon, root access, and a whole ceremony for
  something that isn't a service

The result is the oldest problem in software: *"it works on my machine."*

## How xbin is different

| vs | xbin wins on |
|---|---|
| **AppImage / Snap / Flatpak** | targets **headless web/server/CLI**, not desktop GUI |
| **Docker** | **no daemon, no root, one file** — not an orchestrator |
| **pkg / PyInstaller / nexe** | **language-agnostic** — packages a rootfs, not one language runtime |
| **Go static binaries** | same single-file UX, but for **scripted runtimes** (Python, Node) too |

## How it works

```
┌──────────────────────────────────────────────────────┐
│  my-app.xbin =                                        │
│    [ ELF launcher ][ zstd layers ][ metadata ][ footer ]│
│      Rust/musl        runtime + app    JSON      92B    │
│      ~615KB            layers          entrypoint  v3   │
└──────────────────────────────────────────────────────┘
```

**At build time**, `xbin build`:
1. Detects the runtime (Python, Node, or native binary)
2. Scans Dockerfile for declared system/pip/npm packages and external binary fetches
3. Resolves shared libraries via a pure-Python ELF parser (no host `ldd` needed)
4. Packages interpreter + stdlib + `.so` into a **runtime layer**
5. Packages app code + dependencies into an **app layer**
6. Compresses each layer with `zstd`, assembles the `.xbin`

Both layers are `tar`'ed deterministically (normalized mtime/uid/gid, sorted
entries), so identical content produces identical bytes — this is what
makes the build cache and incremental rebuilds possible.

**At runtime**, the launcher:
1. Opens `/proc/self/exe` (not `argv[0]` — more reliable)
2. Reads the versioned footer at end-of-file, validates magic
3. If signed: verifies the Ed25519 signature — **before anything touches disk**
4. Verifies SHA-256 integrity of the payload
5. Checks the local cache — extracts if missing (atomic `rename()`)
6. `execve()` — replaces itself with the embedded app

### Layered format + incremental rebuild

The payload splits into independent **layers**, similar to Docker:

```
[ stub ][ runtime layer (stable) ][ app layer (volatile) ][ metadata ][ footer ]
```

The **runtime layer** (interpreter + stdlib + `.so`) rarely changes and is
cached separately. Editing app code only rebuilds the small **app layer**.

```
Initial build : ~25s   (compressing runtime layer, ~26MB)
Rebuild (code): ~1.2s  (runtime layer reused from cache — no recompression)
```

Two apps sharing the same runtime share the same build-cache entry.
See the [format spec](/docs/src/reference/format.md).

## CLI reference

```bash
xbin build ./my-app -o my-app.xbin       # analyze + produce .xbin
xbin build ./my-app --isolation 2        # with user namespaces + pivot_root
xbin run   my-app.xbin                   # launch (= ./my-app.xbin)
xbin inspect my-app.xbin                 # show contents without extracting
xbin keygen --key-dir <dir>              # generate an Ed25519 keypair
xbin sign my-app.xbin --key <keyfile>    # sign in place
xbin verify my-app.xbin --trusted-dir <dir>  # verify before you trust it
xbin clean                               # remove extracted cache entries
xbin clean --all                         # wipe everything (incl. build cache)
```

Debug: `XBIN_VERBOSE=1 ./my-app.xbin` shows cold/warm start info.

## Example apps

| Example | What it demonstrates |
|---|---|
| [`hello-web`](/examples/hello-web) | Python stdlib HTTP server — zero dependencies |
| [`bottle-web`](/examples/bottle-web) | Third-party dependency vendored in `.venv` |
| [`bottle-web-pip`](/examples/bottle-web-pip) | `requirements.txt` installed automatically at build time |
| [`hello-node`](/examples/hello-node) | Same CLI, Node.js runtime |

```bash
make example      # builds hello-web
```

## Status

**Phase 1 (MVP) — shipped.** Full pipeline works end-to-end: format,
launcher, builder, CLI, incremental rebuilds.

**Phase 2 (robustness) — nearly done.**
- ✅ Ed25519 signatures (`keygen` / `sign` / `verify`), verified before extraction
- ✅ Trust model (`~/.xbin/trusted-keys/` keyring)
- ✅ Isolation level 2 (user namespaces + `pivot_root`)
- ✅ Pure-Python ELF parser (no host `ldd` dependency)
- ✅ `requirements.txt` → automatic pip-install at build time
- ✅ Node.js end-to-end
- ✅ Self-hosting — `xbin` packages its own CLI using `xbin`
- ✅ Dockerfile dependency detection (apt/apk/pip/npm packages + external binary fetches)
- ✅ CODE_STYLE.md enforcement (ruff/black/clippy configs, `make lint`/`make fmt`)
- ✅ mdbook documentation (rewritten to English)
- 🔜 Seccomp syscall filter (last piece)

**Phase 3 (product) — next.**
- 🔜 Python source AST scanner (detects `subprocess`/`os.system` calls not in Dockerfile)
- 🔜 AI dependency analyzer (combines Dockerfile + AST results)
- 🔜 Manifest mode (`xbin.toml`)
- 🔜 squashfs + mmap (cold start < 100ms)
- 🔜 Cross-arch (aarch64), more runtimes (Ruby, Java/GraalVM, Deno)

See the [full roadmap](/docs/src/roadmap.md) (French).

## Repository structure

```
xbin/
├── stub/              Rust launcher (musl static) + crypto helper
│   └── src/
│       ├── main.rs    self-read → verify → cache → exec
│       └── format.rs  footer parser (sync'd with Python)
├── cli/               Python CLI + builder (stdlib-first)
│   └── xbin/
│       ├── cli.py     build / run / inspect / sign / verify / clean
│       ├── build.py   rootfs construction + assembly
│       ├── format.py  footer writer (sync'd with Rust)
│       └── analyzer/  detection + dependency resolution
│           ├── runtime.py      runtime detection (Python, Node, binary)
│           ├── elf.py          pure-Python ELF shared library parser
│           └── dockerfile.py   Dockerfile dependency extraction
├── examples/          demo apps (Python, Node.js)
├── docs/              mdbook documentation
├── CODE_STYLE.md      coding conventions (42 Norm + Linux kernel style)
└── Makefile
```

## Why now?

- **Local AI is exploding.** Distributing `llama.cpp` + a model + a serving
  layer as one file has no clean solution today — Docker is overkill,
  AppImage is desktop-only, PyInstaller can't handle native binaries.
- **Node has built-in SEA, but it's Node-only.** The industry needs a
  language-agnostic equivalent.
- **Rootless containers are mainstream.** User namespaces make real
  filesystem isolation possible without privileges — exactly what `xbin`
  needs for cross-distro portability.

## Why this team

Solo founder. Low-level developer and independent cybersecurity researcher
— finished 42 School in 2025, since then contributing to open-source
security tooling ([Exegol](https://github.com/ThePorgs/Exegol),
[Caido](https://github.com/caido/caido),
[Payloads All The Things](https://github.com/swisskyrepo/PayloadsAllTheThings))
and building [Toboggan](https://github.com/TednoobOneBinary/Toboggan), a
cross-platform systems tool in Rust. `xbin` sits directly in that space:
systems programming, binary formats, and trust boundaries.

---

## License

MIT — see [LICENSE](LICENSE).

## Security

See [SECURITY.md](SECURITY.md) for the security policy and current posture.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines. PRs welcome!
