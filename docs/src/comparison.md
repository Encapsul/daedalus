# Comparison

## xbin vs alternatives

| Criteria | xbin | AppImage | Docker | pkg/PyInstaller |
|---|---|---|---|---|
| Target | **web/server headless** | desktop GUI | everything (server) | single language |
| Zero installation for user | ✅ | ✅ | ❌ (daemon) | ✅ |
| Language-agnostic | ✅ | ✅ | ✅ | ❌ |
| Single file | ✅ | ✅ | ❌ (image+runtime) | ✅ |
| Auto dependency detection | ✅ (+ AI planned) | ❌ (manual) | ❌ | partial |
| Smart caching | ✅ | partial | ✅ | n/a |
| Rootless isolation | ✅ (level 2) | ❌ | ⚠️ (rootless) | ❌ |
| Built-in signatures | ✅ (Phase 2) | optional | ⚠️ | ❌ |
| Open format | ✅ | ✅ | partial | ❌ |

## Honesty about competition

The market is partially solved **from below**, and it's important to
acknowledge:

- **Go, Rust** already produce a single static binary — `xbin` adds nothing
  there.
- **Node 21+** includes native *Single Executable Applications* (SEA).
- **Python** has PyInstaller, Nuitka, PyOxidizer, shiv, pex.
- **Java** has GraalVM native-image.

`xbin`'s defensible angle is therefore **not** "make a single binary"
(already solved language by language), but:

1. **language-agnostic**: one tool, same UX, regardless of the runtime;
2. **web/server headless**: a niche that AppImage/Snap/Flatpak don't address;
3. **AI-powered hidden dependency detection**: what no static tool can do
   (subprocess, `dlopen`, plugins) — see
   [Dependency detection](./guides/dependencies.md);
4. **the local AI model use case**: multi-GB payloads + native binaries +
   server, with no clean solution today.

## The question to always answer

> *"Why not AppImage / Docker / pkg?"*

One-sentence answer: **AppImage is for desktop, Docker is heavy and needs a
daemon, pkg/PyInstaller are mono-language — xbin is for headless web servers,
language-agnostic, in a single self-executing file.**
