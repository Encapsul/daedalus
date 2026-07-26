# Roadmap

## Phase 1 — MVP functional ✅

- [x] `.xbin` format defined and stable (versioned 84B footer)
- [x] Rust/musl static launcher: self-read, SHA-256 integrity, exec
- [x] Atomic cache extraction using `rename()`
- [x] Python builder: runtime detection, rootfs construction, assembly
- [x] CLI: `build`, `run`, `inspect`, `clean`
- [x] Python support (stdlib) — `hello-web` example
- [x] `flock()` for concurrent cache access
- [x] `xbin clean`
- [x] Python `site-packages` / `.venv` support — `bottle-web` example
- [x] **v2 layered format + incremental rebuild** (runtime reused, rebuild ~25s → ~1s; shared build cache between apps)
- [x] `requirements.txt` → pip install at build time (temp venv)
- [x] Node.js end-to-end support (stdlib + node_modules)
- [x] Deno support (deno.json detection, `--allow-all`, tasks-based entrypoint)

## Phase 2 — Robustness ✅

- [x] **Pure-Python ELF analyzer** (no host `ldd` dependency, works on any machine with Python)
- [x] **Ed25519 signatures** (v3 footer, `xbin keygen` / `sign` / `verify` / `trust`)
- [x] **Trust model**: `$XDG_DATA_HOME/xbin/trusted-keys/` keyring, `xbin trust` subcommand
- [x] **Level 2 isolation**: user namespaces + `pivot_root` (real portability)
- [x] **Smart `.so` deduplication** (removes duplicate `ld-linux` instances)
- [x] `/etc/hosts` in rootfs (prevents DNS PTR lookup hang)
- [x] **Self-hosting**: `self/` → `xbin build self/` → `./xbin build ...` (full cycle: CLI → `xbin-v2` → `xbin-v3` → `xbin-v3`)
- [x] **Dockerfile dependency detection** (apt/apk/pip/npm packages + external binary fetch chains)
- [x] **Python AST scanner** (subprocess/os.system call detection)
- [x] **Dependency fetcher** (isolated staging, non-invasive)
- [x] **PATH injection** (bundled binaries found at runtime)
- [x] **Minimal seccomp filter** (denylist of 16 dangerous syscalls, installed after pivot_root, arch-aware for x86_64 and aarch64)
- [x] **Payload encryption** (AES-256-GCM, v4 format, HKDF key derivation from signing seed, decrypt after sig+integrity verification)
- [ ] Manifest mode (`xbin.toml`) for complex dependencies
- [ ] LRU cache cleanup (evict beyond threshold)

## Phase 3 — Production ✅

- [x] **SquashFS extraction** (v5 format, `--squashfs` flag, `mksquashfs` build, `backhand` Rust parser, better compression than zstd+tar)
- [x] **11 supported runtimes**: Python, Node.js, Deno, Java, Ruby, .NET/C#, Go, PHP, Perl, Binary, Hugo
- [x] **Framework auto-detection**: Next.js, Nuxt, Astro, Remix, SvelteKit, Express, Fastify, Hono, Django, FastAPI, Flask, Laravel, Symfony
- [x] **`.env` file baking** (`--env-file` flag, secret detection)
- [x] **Version metadata** (`--version-info`, `--author`, `--description`, `--license`)
- [x] **Persistent storage** (`--persist` flag, `XBIN_PERSIST_DIR` env var)
- [x] **Data files** (`--include PATH` flag, repeatable)
- [x] **Tree-shaking** (`--tree-shake`, removes unused node_modules)
- [x] **Minification** (`--minify`, JS/TS via terser, CSS built-in)
- [x] **Health checks** (`--health-port`, `/healthz`, `/readyz`, `/status`)
- [x] **OpenTelemetry** (`--otel-endpoint`, OTLP export, auto-instrumentation)
- [x] **Cron/scheduled tasks** (`--cron NAME:SCHEDULE`, background scheduler)
- [ ] **squashfs + mmap**: direct read, no extraction (kernel mount, Linux 5.12+)
- [ ] **Cold/warm start < 100 ms** end-to-end
- [ ] Distribution / discovery (lightweight registry, even P2P)