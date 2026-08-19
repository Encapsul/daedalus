# Positioning

`erebus`'s positioning is its most important decision. It defines what we
build — and more importantly, what we don't.

## erebus targets web/server headless, not desktop

| | Desktop GUI | Web / Server Headless |
|---|---|---|
| Examples | VLC, Inkscape, GIMP | Next.js, FastAPI, CLI build tool |
| Needs | X11/Wayland, desktop integration, icons | a network port, stdout/stderr |
| Existing solution | **AppImage, Snap, Flatpak** | **nothing clean and language-agnostic** |

AppImage and friends spent years solving desktop integration. This is not
the territory nor the ambition of `erebus`. `erebus`'s niche is what those tools
don't target: *"I launch the binary, a server starts, I open my browser."*

## Why not pkg / nexe / PyInstaller?

These exist but are **mono-language**:

- `vercel/pkg`, `nexe` → Node only;
- `PyInstaller`, `Nuitka`, `PyOxidizer` → Python only;
- `GraalVM native-image` → JVM only.

`erebus` is **language-agnostic** by design: it packages a *rootfs* (a
mini-filesystem), not a specific language. The same tool packages a Python
app, a Node app, or a native binary.

## What erebus is not

- **Not a Docker killer.** Docker remains for orchestration and
  multi-container.
- **Not a VM.** We don't virtualize the kernel; the app runs on the host
  kernel.
- **Not a package manager.** No central registry (for now).
- **Not "one standard to rule them all".** See [XKCD 927](#xkcd-927) below.

## XKCD 927: we know the trap

> *"There are 14 competing standards."*
> *"14?! Ridiculous! We need to develop one universal standard that covers all
> use cases."*
> *"There are 15 competing standards."*

— [XKCD 927: Standards](https://xkcd.com/927/)

We are **aware** that every "universal format" proposal risks becoming one
more incompatible format. erebus is itself a packaging format standing next to
AppImage, Snap, Flatpak, deb, rpm, Docker, pkg, PyInstaller... A naive
reaction would be "build a universal packaging format" — and that is exactly
how you end up with a 15th (or 12th) format.

### How we avoid becoming "the 12th format"

1. **Narrow scope, not universal.** erebus explicitly targets **headless
   web/server apps**, the niche AppImage/Snap/Flatpak don't serve. It does
   not try to replace desktop packaging or container orchestration. Scope
   discipline is our anti-XKCD measure.
2. **Reuse existing standards.** The `.ere` file is a valid ELF (runs with
   `chmod +x`), the payload is standard `tar`/`zstd`/SquashFS, signatures are
   standard Ed25519, and the rootfs is a plain POSIX filesystem — no
   proprietary kernel features, no new archive format. What is novel is the
   packaging pipeline, not a new on-disk container standard.
3. **One evolving format, not many.** New features extend the existing
   `.ere` format backward-compatibly (v2 → v3 → v4 → v5), never a fresh
   incompatible format. See [`reference/format.md`](../reference/format.md).
4. **Eat our own dogfood.** erebus packages apps *without* requiring a daemon,
   root, or installation — reducing the incentive to switch to a
   "new standard" that "fixes" a deployment pain.
5. **Interop over lock-in.** A `.ere` can be unpacked with standard tools
   (`tar`, `unsquashfs`, `zstd`); no proprietary reader required to access
   the payload.

If a future feature cannot be added without breaking the format, we bump the
version *and* keep reading older versions — we never fork the format or
introduce a parallel one.

> The right positioning: **"for cases where Docker is too heavy and where
> AppImage doesn't apply, erebus is the answer."**

## The most promising use case

Distributing **local AI models**: packaging `llama.cpp` + a multi-GB model +
an inference server + a web UI, in a single file that launches with
`./my_llm`. Today this case has no clean solution (Docker is heavy, AppImage
is not designed for multi-GB payloads, PyInstaller can't handle C binaries).
`erebus`'s architecture is naturally suited for it.
