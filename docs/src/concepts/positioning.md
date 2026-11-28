# Positioning

daedalus packages an application and its runtime into one self-extracting
file. It is for developers who ship a web server, an API or a CLI to machines
that have nothing installed: no Docker, no Python, no Node. The file can be
signed to prove where it came from and updated with signed deltas.

## Compared with other tools

| | Docker | PyInstaller / pkg | llamafile | **daedalus** |
|---|---|---|---|---|
| Runtime | Container daemon | One language | C++ only | Several runtimes detected; CI smokes Python stdlib |
| Delta updates | Layer cache (partial) | None | None | SISR in the code; the published figure is a synthetic blob |
| Isolation | Namespaces | None | None | User namespaces and a seccomp denylist (18 syscalls) |
| Portable format | OCI image | Executable | Single file | Linux ELF (`.daedalus`, alias `.de`) |
| Self-contained | Needs a daemon | Runtime bundled | All in one binary | Stub plus a rootfs extracted to `~/.cache` |

**llamafile** packages a model with llama.cpp to run that model. daedalus
packages a whole application (web server, API, runtime) and can carry a model
as one of its files.

**Docker** is built for orchestration. It needs a daemon, root or user
namespaces, and network access to pull the image. A `.daedalus` is one
executable file: atomic extraction, no daemon, no root.

**PyInstaller and pkg** handle one language each. daedalus can hold a Python
API, a native binary and a Node worker in one file. That multi-runtime case
has no CI smoke test yet; the only CI smoke is `examples/hello-web`.

## When to use something else

- **Orchestration and several containers:** Docker stays simpler.
- **A different kernel:** use a VM. daedalus runs on the host kernel.
- **Desktop integration** (icons, menus, X11/Wayland): AppImage, Snap or
  Flatpak. daedalus targets headless web and server apps: run the file, a
  server starts, open the browser.
- **Fastest cold start on a small Node app:** AppImage and pkg win, because
  daedalus extracts its payload on first launch.
- **Compiling Python to WASM or bridging languages:** daedalus does not do
  that. Each runtime runs in its own environment.

## Not a new format

A `.daedalus` file is:

- a valid ELF on Linux: `chmod +x && ./app.daedalus` works;
- a tar/zstd or SquashFS payload you can open with `tar` or `unsquashfs`;
- standard Ed25519 signatures you can check with any crypto library;
- a POSIX rootfs with no proprietary kernel features.

The format went from v2 (plain) to v3 (signed), v4 (encrypted), v5 (SquashFS)
and v6 (the signature also covers the stub). Older versions stay readable.

## Local models

daedalus can carry a model the same way it carries any other file:

- it detects Ollama and Gemma layouts (`Modelfile`, `models/*.gguf`);
- the weights are in the payload only if they were in the tree at build time;
- a signed SISR delta can update them later. Measure it with
  `DAEDALUS_SISR_MODEL_V1` / `DAEDALUS_SISR_MODEL_V2` and the ignored test
  `gemma_weight_delta_bandwidth`. The synthetic 89.7% figure is not a model
  result.

`examples/local-model-kit` holds real weights: `fetch_weights.sh` pulls a
Gemma 3 270M Q4 GGUF from Hugging Face into the gitignored `models/` folder.
`examples/offline-health-agri` is a scaffold: it has a `Modelfile` and no
weights, so it is not an offline model on its own.
