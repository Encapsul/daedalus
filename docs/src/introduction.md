# x.bin

> *Ship your web app like a binary. Run anywhere.*

**x.bin** — the `x` for *any app*, the `.bin` for *a binary*.
The CLI tool is called `xbin` and the output files use the `.xbin`
extension (a dot cannot appear in a shell command name).

`x.bin` turns any **web / server / headless** application into **a single
self-contained executable file**. The end user installs nothing:

```bash
chmod +x my_app.xbin && ./my_app.xbin
# [xbin] starting app...
# Server listening on http://127.0.0.1:8080
```

No runtime to install. No `node` or `python` on the target machine. No
Docker. One file, make it executable, run it.

## In one sentence

`xbin` is to a server app what a Go static binary is to a compiled program:
**everything is inside, it runs everywhere.**

## What's different

AppImage, Snap and Flatpak solve this for **desktop GUI** apps (they need
X11, desktop integration, icons...). `xbin` targets the opposite and widely
ignored angle: **web and server headless** apps — a Next.js server, a FastAPI
API, a CLI build tool. You launch the binary, the server starts, you open
your browser.

And unlike `vercel/pkg` or `nexe` (Node only), `xbin` is **language-agnostic**:
Python, Node, Go, native binaries.

## Project status

Phase 2 complete, Phase 3 partially done. The full pipeline runs end-to-end:
`build` → `.xbin` → execution with self-extraction, cache, Ed25519 signatures,
squashfs support, and cross-compilation (`--target aarch64`). See the
[Roadmap](./roadmap.md) for what's next.
