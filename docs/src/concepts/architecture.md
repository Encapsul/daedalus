# Architecture

`daedalus` is split into 4 layers with clear interfaces between them, allowing
each layer to evolve independently (e.g. switching from tar extraction to
squashfs+mmap) without rewriting everything.

```
┌──────────────────────────────────────┐
│            CLI (daedalus)                │  build · run · inspect · clean
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
│            Format .ere               │  shared binary spec
└──────────────────────────────────────┘
```

Each layer only knows the one below it. The **`.ere` format** is the shared
contract: the builder (Rust) writes it, the launcher (Rust) reads it. As
long as the format is respected, both sides evolve independently.

## Architecture diagram (initial sketch)

The diagram below is the **target architecture** envisioned for end of
Phase 2/3, not the current MVP state.

![daedalus Architecture](../images/architecture.png)

### Reading the diagram

The diagram flows top to bottom:

1. **CLI** (`daedalus ./my_app`) — four commands: *Build · Run · Inspect ·
   Clean*. This is the user surface.

2. **Builder** — two sub-components:
   - **Analyzer**: Rust ELF parser (DT_NEEDED, DT_RUNPATH, transitive
     resolution), runtime detection, and hidden dependency detection
     (subprocess, `dlopen`) — see the *AI* annotation on the right.
   - **Packager**: builds the rootfs, compresses with zstd, assembles the
     final `.ere`.
   - The **AI** annotation (top right) marks the intended role of AI:
     *analyze source code, detect subprocess and dlopen calls invisible to
     static analysis*. This is the project's differentiator — see
     [Dependency detection](../guides/dependencies.md).

3. **`.ere` Format** — the central layer: *ELF Launcher · zstd Payload ·
   JSON Metadata · Magic + SHA-256*. This is the file format described in
   [reference](../reference/format.md).

4. **Runtime** — two sub-components:
   - **Cache**: `~/.cache/daedalus/{sha256}/`, single extraction, `flock()` for
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
| Rust ELF analyzer | ✅ | ✅ implemented (replaces host `ldd`) |
| Dockerfile dependency detection | ✅ | ⚠️ removed (Python CLI only) |
| Python AST scanner (subprocess) | ✅ | ⚠️ removed (Python CLI only) |
| Dependency fetcher (staging) | ✅ | ⚠️ removed (Python CLI only) |
| PATH injection (bundled binaries) | ✅ | ✅ implemented (rootfs usr/bin prepended) |
| AI analyzer (hidden deps) | ✅ | ⏳ Phase 3 |
| ELF + zstd + meta + SHA-256 format | ✅ | ✅ implemented |
| `{sha256}` cache + atomic extraction | ✅ | ✅ implemented (`flock()` included) |
| Level 0 executor (`LD_LIBRARY_PATH` + `PATH`) | ✅ | ✅ implemented |
| User namespaces + pivot_root + seccomp | ✅ | ✅ implemented (Phase 2) |
| Ed25519 signatures | ✅ | ✅ implemented (Phase 2) |
| warm start < 100 ms | ✅ | ⏳ (currently limited by embedded runtime boot, not by daedalus) |

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
  requirements.txt   →  daedalus build  →   my_app.ere   →  ./my_app.ere  →  it runs
+ python3                              (1 file)
+ .so libs
```

`daedalus build` does the hard work **once**. The end user only sees a single
file.

## Self-reconstruction (SISR, opt-in)

The architecture above is the **static** model. An **optional** extension adds
a third responsibility inside the binary — the embedded SISR engine — which
lets the binary update itself from signed deltas, with no toolchain on the
target machine. Both models ship in the same file shape and behave identically
on a default launch; SISR only engages on an explicit update.

```
+-----------------------------------------------------------------+
|                         APP.ERE (ELF)                          |
+-----------------------------------------------------------------+
|  1. Entrypoint Launcher  --> Bootstrap + runtime isolation       |
+-----------------------------------------------------------------+
|  2. Embedded SISR Engine --> Verify, fetch & assemble (dormant)  |
+-----------------------------------------------------------------+
|  3. Payload (SquashFS)   --> Application code & assets           |
+-----------------------------------------------------------------+
```

The reconstruction flow replaces the "redistribute a new binary" step with a
**self-rebuild on the target**:

```
[ Binary v1.0 ] ──( ./app.ere update )──▶ [ Interrogate remote manifest ]
                                                │
                                                ▼
                                       [ Download deltas / chunks ]
                                                │
                                                ▼
                                   [ Validate Ed25519 signatures + hashes ]
                                                │
                                                ▼
[ Binary v1.1 ] ◀──( Rebuild & atomic swap )───┘
```

Every step is verified: signature before anything runs, anti-rollback index
before applying, per-block SHA-256 and a Merkle commitment before commit. An
interruption leaves `v1.0` intact.

| Layer | Static `.ere` | SISR `.ere` |
|---|---|---|
| Launcher | same | same (unchanged) |
| SISR engine | absent | embedded, dormant by default |
| Payload (SquashFS) | same | same |
| Update path | rebuild + redistribute | self-rebuild from signed deltas |

SISR is described in detail in the [SISR overview](./sisr-overview.md), the
[delta manifest spec](../spec/delta-manifest-format.md), and the
[incremental updates guide](../guides/incremental-updates.md).
