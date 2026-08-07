# x.bin

> **Ship your web app as a binary. Run it anywhere.** No runtime, no Docker,
> no install step on the target machine — one file that runs.

**x.bin** packages any web, server, or headless app into a single
self-extracting ELF binary. The CLI is `xbin`; its output files use the
`.xbin` extension (a dot can't appear in a shell command name).

xbin is to a server app what a Go static binary is to a compiled program:
everything lives inside the file, and it runs with `./my_app.xbin`.

## What x.bin does

```bash
# Build once — bundles the runtime, the app, and its libraries
$ xbin build ./my-app -o my-app.xbin

# Run anywhere — the end user installs nothing
$ ./my-app.xbin
# Server listening on http://127.0.0.1:8080
```

| Step | What happens | Who does it |
|---|---|---|
| `xbin build` | detects the runtime, resolves dependencies, compresses a rootfs | the developer |
| `./my_app.xbin` | verifies integrity, extracts to cache, launches | the end user |

## Key features

- **Language-agnostic** — Python, Node.js, Java, Ruby, .NET/C#, Deno, Go,
  PHP, Perl, and native binaries, all through the same CLI and file format.
- **Zero install for the end user** — no `node`, `python`, or Docker on the
  target machine.
- **Incremental builds** — the runtime layer is cached; app-only edits rebuild
  in about a second.
- **Integrity first** — every `.xbin` carries a `SHA-256` of its payload,
  verified before anything is extracted. Optional Ed25519 signatures and
  AES-256-GCM encryption.
- **Sandboxing without root** — user namespaces, mount namespaces,
  `pivot_root`, and a seccomp denylist for the strongest isolation level.

## When to use x.bin

| Use case | Example |
|---|---|
| Distribute a server app without a runtime install | a FastAPI API, a Next.js server |
| Ship internal CLI/build tools to heterogeneous machines | a deploy tool that must "just run" |
| Bundle a dependency-heavy app into one artifact | Python with `requirements.txt`, Node with `node_modules` |
| Version-pin the runtime with the app | app written against a specific Python/Node release |

x.bin targets **web, server, and headless** apps. It is not a fit for:

- **Desktop GUI apps** — AppImage, Snap, and Flatpak already target those
  (X11/Wayland integration, icons, desktop entry).
- **Per-process isolation you don't control** — use containers when you want
  the *host* to remain completely untouched; x.bin's sandbox is opt-in per app.

## How it differs

| Tool | Scope | Runtime |
|---|---|---|
| `vercel/pkg` / `nexe` | Node.js only | embeds Node |
| PyInstaller | Python only | embeds CPython |
| AppImage / Snap / Flatpak | desktop GUI apps | needs desktop integration |
| **x.bin** | **any language, server apps** | **self-contained, no install** |

## Get started

```bash
# Prerequisites: Rust with the musl target, zstd, a C compiler
make preflight

# Build the launcher, then package your first app
make stub
xbin build ./examples/hello-web -o hello-web.xbin
./hello-web.xbin
```

Follow the [Quickstart](./guides/quickstart.md) for the full walkthrough, or
jump straight to a language guide: [Python](./guides/python.md),
[Node.js](./guides/node.md), [Go](./guides/go.md).

## Project status

Phase 2 is complete; Phase 3 is in progress. The full pipeline runs
end-to-end: `build` → `.xbin` → execution with self-extraction, caching,
Ed25519 signatures, SquashFS support, and multi-arch builds
(`x86_64` / `aarch64`). See the [Roadmap](./roadmap.md) for what's next.
