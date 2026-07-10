# Positioning

`xbin`'s positioning is its most important decision. It defines what we
build — and more importantly, what we don't.

## xbin targets web/server headless, not desktop

| | Desktop GUI | Web / Server Headless |
|---|---|---|
| Examples | VLC, Inkscape, GIMP | Next.js, FastAPI, CLI build tool |
| Needs | X11/Wayland, desktop integration, icons | a network port, stdout/stderr |
| Existing solution | **AppImage, Snap, Flatpak** | **nothing clean and language-agnostic** |

AppImage and friends spent years solving desktop integration. This is not
the territory nor the ambition of `xbin`. `xbin`'s niche is what those tools
don't target: *"I launch the binary, a server starts, I open my browser."*

## Why not pkg / nexe / PyInstaller?

These exist but are **mono-language**:

- `vercel/pkg`, `nexe` → Node only;
- `PyInstaller`, `Nuitka`, `PyOxidizer` → Python only;
- `GraalVM native-image` → JVM only.

`xbin` is **language-agnostic** by design: it packages a *rootfs* (a
mini-filesystem), not a specific language. The same tool packages a Python
app, a Node app, or a native binary.

## What xbin is not

- **Not a Docker killer.** Docker remains for orchestration and
  multi-container.
- **Not a VM.** We don't virtualize the kernel; the app runs on the host
  kernel.
- **Not a package manager.** No central registry (for now).

> The right positioning: **"for cases where Docker is too heavy and where
> AppImage doesn't apply, xbin is the answer."**

## The most promising use case

Distributing **local AI models**: packaging `llama.cpp` + a multi-GB model +
an inference server + a web UI, in a single file that launches with
`./my_llm`. Today this case has no clean solution (Docker is heavy, AppImage
is not designed for multi-GB payloads, PyInstaller can't handle C binaries).
`xbin`'s architecture is naturally suited for it.
