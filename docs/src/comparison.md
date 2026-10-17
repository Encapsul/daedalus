# Comparison

## daedalus vs alternatives

| Criteria | daedalus | AppImage | Docker | pkg/PyInstaller |
|---|---|---|---|---|
| Target | **web/server headless** | desktop GUI | everything (server) | single language |
| Zero installation for user | ✅ | ✅ | ❌ (daemon) | ✅ |
| Language-agnostic | ✅ | ✅ | ✅ | ❌ |
| Single file | ✅ | ✅ | ❌ (image+runtime) | ✅ |
| Auto dependency detection | ✅ (pkgmgr detection) | ❌ (manual) | ❌ | partial |
| Smart caching | ✅ | partial | ✅ | n/a |
| Rootless isolation | ✅ (level 2) | ❌ | ⚠️ (rootless) | ❌ |
| Built-in signatures | ✅ (Phase 2) | optional | ⚠️ | ❌ |
| Open format | ✅ | ✅ | partial | ❌ |

## What daedalus is

Go and Rust already produce a single static binary - daedalus adds nothing there. Node 21+ includes native Single Executable Applications (SEA). Python has PyInstaller, Nuitka, PyOxidizer, shiv, pex. Java has GraalVM native-image.

daedalus's angle is not "make a single binary" (already solved language by language), but:

1. **language-agnostic**: one tool, same UX, regardless of the runtime;
2. **web/server headless**: a niche that AppImage/Snap/Flatpak don't address;
3. **smart dependency detection**: detects package managers (pip, npm, pnpm, etc.) and install commands from the app directory - see [Dependency detection](./guides/dependencies.md);
4. **single-file distribution**: one ELF for the app you built. SISR can send a
   delta. The only size ratio checked into this repo is a synthetic 200 MiB
   buffer (89.7% fewer bytes after a 10% perturbation), not a production update.

## The question to always answer

> *"Why not AppImage / Docker / pkg?"*

One-sentence answer: **AppImage and pkg are faster and smaller on a hello-node (29 ms and 109 ms versus 2.1 s for daedalus, one measured pass). Docker needs a daemon. daedalus is the ELF for a headless app whose runtime you want copied in at build time, with a signature a second machine can check.**