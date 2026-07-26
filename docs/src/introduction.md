# x.bin

> *Ship your web app as a binary. Run it anywhere.*

**x.bin** is the `x` for any app, and `.bin` for a binary. The CLI tool is `xbin`, and its output files use the `.xbin` extension (a dot can't be in a shell command name).

xbin packages any web, server, or headless app into a single executable file. The end user installs nothing.

No runtime is required. No need for `node`, `python`, or Docker. One file — make it executable and run it.

xbin is to a server app what a Go static binary is to a compiled program: everything is inside, and it runs everywhere.

How it differs
AppImage, Snap, and Flatpak target desktop GUI apps (requiring X11, desktop integration, and icons). xbin targets the opposite: web and server headless apps — like a Next.js server, a FastAPI API, or a CLI build tool. You launch the binary, the server starts, and you open your browser.

Unlike `vercel/pkg` or `nexe` (Node only), xbin is language-agnostic: Python, Node.js, Java, Ruby, .NET/C#, Deno, Go, PHP, Perl, or native binaries — same CLI, same format.

## Project status

Phase 2 is complete, and Phase 3 is partially done. The full pipeline runs end-to-end:
`build` → `.xbin` → execution with self-extraction, cache, Ed25519 signatures, squashfs support, and cross-compilation (`--target aarch64`). See the [Roadmap](./roadmap.md) for what's next.