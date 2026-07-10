# Architecture

`xbin` is split into 4 layers with clear interfaces between them, allowing
each layer to evolve independently (e.g. switching from tar extraction to
squashfs+mmap) without rewriting everything.

```
┌──────────────────────────────────────┐
│            CLI (xbin)                │  build · run · inspect · clean
├──────────────────────────────────────┤
│            Builder                    │  Analyzer + Packager
│   ┌──────────────┬──────────────┐     │
│   │   Analyzer   │   Packager   │     │
│   └──────────────┴──────────────┘     │
├──────────────────────────────────────┤
│            Runtime (launcher)         │  self-read · cache · executor
│   ┌──────────────┬──────────────┐     │
│   │    Cache     │   Executor   │     │
│   └──────────────┴──────────────┘     │
├──────────────────────────────────────┤
│            Format .xbin               │  shared binary spec
└──────────────────────────────────────┘
```

Each layer only knows the one below it. The **`.xbin` format** is the shared
contract: the builder (Python) writes it, the launcher (Rust) reads it. As
long as the format is respected, both sides evolve independently.

## Architecture diagram (initial sketch)

The diagram below is the **target architecture** envisioned for end of
Phase 2/3, not the current MVP state.

![xbin Architecture](../images/architecture.png)

### Reading the diagram

The diagram flows top to bottom:

1. **CLI** (`xbin ./my_app`) — four commands: *Build · Run · Inspect ·
   Clean*. This is the user surface.

2. **Builder** — two sub-components:
   - **Analyzer**: pure-Python ELF parser (DT_NEEDED, DT_RUNPATH, transitive
     resolution), runtime detection, and hidden dependency detection
     (subprocess, `dlopen`) — see the *AI* annotation on the right.
   - **Packager**: builds the rootfs, compresses with zstd, assembles the
     final `.xbin`.
   - The **AI** annotation (top right) marks the intended role of AI:
     *analyze source code, detect subprocess and dlopen calls invisible to
     static analysis*. This is the project's differentiator — see
     [Dependency detection](../guides/dependencies.md).

3. **`.xbin` Format** — the central layer: *ELF Launcher · zstd Payload ·
   JSON Metadata · Magic + SHA-256*. This is the file format described in
   [reference](../reference/format.md).

4. **Runtime** — two sub-components:
   - **Cache**: `~/.cache/xbin/{sha256}/`, single extraction, `flock()` for
     concurrent access, LRU cleanup.
   - **Executor**: *Linux user namespaces · pivot_root (isolation) · seccomp
     filter · LD_LIBRARY_PATH fallback*.

5. **Goal** (bottom): *single binaries · warm start < 100 ms · 0
   dependencies*.

### Gap between target diagram and current MVP

The diagram describes the **ambition**. Here is the honest current state:

| Element | Target | Current |
|---|---|---|
| CLI build/run/inspect | ✅ | ✅ implemented |
| Pure-Python ELF analyzer | ✅ | ✅ implemented (replaces host `ldd`) |
| AI analyzer (hidden deps) | ✅ | ⏳ Phase 3 |
| ELF + zstd + meta + SHA-256 format | ✅ | ✅ implemented |
| `{sha256}` cache + atomic extraction | ✅ | ✅ implemented (`flock()` included) |
| Level 0 executor (`LD_LIBRARY_PATH`) | ✅ | ✅ implemented |
| User namespaces + pivot_root + seccomp | ✅ | ✅ pivot_root implemented (Phase 2) |
| Ed25519 signatures | ✅ | ✅ implemented (Phase 2) |
| warm start < 100 ms | ✅ | ⏳ (currently limited by embedded runtime boot, not by xbin) |

> **Design note.** The diagram places isolation (namespaces, seccomp) at the
> core of the runtime. In practice we chose to **start at level 0**
> (`LD_LIBRARY_PATH`, no isolation) because isolation is a *feature*, not the
> core value proposition. The value is *"one file, it runs"*. Isolation is
> added incrementally, without changing the format.

## The full flow in one picture

```
DEV MACHINE                          TARGET MACHINE

my_app/
  app.py
  requirements.txt   →  xbin build  →   my_app.xbin   →  ./my_app.xbin  →  it runs
+ python3                              (1 file)
+ .so libs
```

`xbin build` does the hard work **once**. The end user only sees a single
file.
