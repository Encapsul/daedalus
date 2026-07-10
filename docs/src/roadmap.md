# Roadmap

## Phase 1 — MVP functional ✅

- [x] `.xbin` format defined and stable (versioned 84B footer)
- [x] Rust/musl static launcher: self-read, SHA-256 integrity, exec
- [x] Atomic cache extraction (`rename()`)
- [x] Python builder: runtime detection + rootfs construction + assembly
- [x] CLI: `build`, `run`, `inspect`, `clean`
- [x] Python support (stdlib) — `hello-web` example
- [x] `flock()` for concurrent cache access
- [x] `xbin clean`
- [x] Python `site-packages` / `.venv` support — `bottle-web` example
- [x] **v2 layered format + incremental rebuild** (runtime reused,
      rebuild ~25s → ~1s; shared build cache between apps)
- [x] `requirements.txt` → pip install at build time (temp venv)
- [x] Node.js end-to-end support (stdlib + node_modules)

## Phase 2 — Robustness ✅

- [x] **Pure-Python ELF analyzer** (no host `ldd` dependency, works on any
      machine with Python)
- [x] **Ed25519 signatures** (v3 footer, `xbin keygen` / `sign` / `verify` / `trust`)
- [x] **Trust model**: `~/.xbin/trusted-keys/` keyring, `xbin trust` subcommand
- [x] **Level 2 isolation**: user namespaces + `pivot_root` (real portability)
- [x] **Smart `.so` deduplication** (if `ld-linux` is found via `/lib64`
      (symlink) and `/lib/x86_64-linux-gnu` (real file), the duplicate is
      automatically removed)
- [x] `/etc/hosts` in rootfs (prevents DNS PTR lookup hang)
- [x] **Self-hosting**: `self/` → `xbin build self/` → `./xbin build ...`
      (full cycle: CLI → `xbin-v2` → `xbin-v3` → `xbin-v4`)
- [ ] Minimal seccomp filter
- [ ] Manifest mode (`xbin.toml`) for complex dependencies
- [ ] AI analyzer: generate `xbin.toml` (hidden deps: subprocess, dlopen)
- [ ] LRU cache cleanup (evict beyond threshold)

## Phase 3 — Production

- [ ] **squashfs + mmap**: direct read, no extraction
- [ ] **Cold/warm start < 100 ms** end-to-end
- [ ] All runtime support (Java/GraalVM, Ruby, etc.)
- [ ] Cross-arch (aarch64)
- [ ] Distribution / discovery (lightweight registry, even P2P)

## Guiding principle

Each phase must be addable **without rewriting** the previous one. This is
why the format is versioned and the layers are decoupled: switching from tar
extraction to squashfs+mmap, or from level 0 to level 2, doesn't change the
contract between builder and launcher.
