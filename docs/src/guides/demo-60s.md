# The 60-second demo

daedalus makes one Linux ELF you can run. The demo that matches the CI smoke
is a Python standard-library HTTP server. It does not embed a model.

## The script

```bash
# Install. The script checks sha256 against checksums.txt and stops if it cannot.
curl -fsSL https://raw.githubusercontent.com/Encapsul/daedalus/main/install.sh | sh

# Package the app CI builds on every Linux run
daedalus build ./examples/hello-web -o hello-web.daedalus

# Run it
./hello-web.daedalus
```

On another terminal:

```bash
curl -s http://127.0.0.1:8080
```

Stop there. Wall-clock for this example depends on the machine. Do not quote
the old "6.2 s / 16.9 MB / Gemma" line. That artifact did not contain a model.

## What the file actually is

- A Linux ELF. macOS and Windows need WSL or a Linux VM to run it.
- The stub checks SHA-256 before it extracts into `~/.cache/daedalus/<hash>/`.
- The first launch pays for that extract. On one measured hello-node
  (312 bytes of JavaScript, Node 24, machine `codespaces-ba69ea`): artifact
  44 MiB, 122.6 MiB on disk after extract, 2.1 s to the first HTTP 200.
  AppImage on the same pass was 29 ms and 24.4 MiB. pkg was 109 ms. Docker
  was 6.1 s and needs a daemon. Warm start for daedalus was 96 ms.
  See [the comparison](../../../benchmarks/comparison/README.md).
- `daedalus verify hello-web.daedalus` on the build machine succeeds because
  the default dev key is trusted there. Another machine needs `daedalus trust`
  of the `.pub` file first.

## The clinic example is a scaffold

`examples/offline-health-agri` is a standard-library script and a `Modelfile`.
There are no model weights in the tree. `diagnose` prints hardcoded checks,
then asks Ollama on `localhost:11434` if something is already listening.
If nothing is, it prints the prompt and exits. It is not a packaged Gemma.

SISR delta size on a real weight pair is an ignored test
(`gemma_weight_delta_bandwidth`). The 89.7% figure is a synthetic 200 MiB
file with 10% of its bytes changed.

## Replay

```bash
daedalus inspect hello-web.daedalus
daedalus scan examples/hello-web
```
