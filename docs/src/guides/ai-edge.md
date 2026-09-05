# AI and edge runtimes

daedalus can package AI and edge workloads, including Ollama models and MCP
tools for agents.

## Gemma via Ollama (on-device AI)

daedalus can package applications that use [Gemma](https://github.com/google/gemma),
Google's open-weights model, running locally via [Ollama](https://ollama.ai) or
[llama.cpp](https://github.com/ggerganov/llama.cpp).

**Use case: Offline code analysis assistant**

1. Write a Python/Node app that uses the `daedalus-core` hidden-dependencies
   analyzer to scan source code for subprocess/dlopen calls
2. The analyzer runs Gemma (via Ollama) locally for LLM-powered classification
3. Package the app with `daedalus build . -o app.daedalus`
4. The `.daedalus` binary embeds Ollama + the Gemma model
5. On first run, Ollama starts and serves the analyzer on `http://127.0.0.1:PORT`
6. The app performs offline code analysis — no internet, no cloud, no cost

```bash
# Build the app with Ollama embedded
daedalus build ./my-code-assistant -o code-assistant.daedalus

# First run: Ollama auto-starts, Gemma model loads
./code-assistant.daedalus

# Then analyze any codebase offline
code-assistant.daedalus scan /path/to/codebase
```

## Ollama

Package an Ollama-based AI app:

```bash
daedalus build ./my-ollama-app -o my-ollama-app.daedalus
```

daedalus detects:

- `ollama` in `package.json` scripts or dependencies
- `Modelfile` or `models/` directory

The binary embeds the Ollama binary and model files. At runtime, it starts the
Ollama server and serves the app on the configured port.

## MCP tools for agents

daedalus can expose Model Context Protocol (MCP) tools:

```bash
daedalus build ./my-agent -o my-agent.daedalus \
  --mcp-tools ./tools/
```

MCP tools are embedded as standalone binaries or scripts and exposed over
stdin/stdout JSON-RPC.

## GPU passthrough (accelerated inference)

Ollama and llama.cpp offload layers to the host GPU when the right device
nodes and `CUDA_VISIBLE_DEVICES`/`HIP_VISIBLE_DEVICES` are visible. daedalus
records the compute backend at build time and the launcher wires it into the
sandbox automatically:

```bash
# Auto-detect the build machine's backend (NVIDIA or ROCm)
daedalus build ./my-ollama-app -o llm.daedalus --gpu auto

# Force a specific backend
daedalus build ./my-ollama-app -o llm.daedalus --gpu nvidia
daedalus build ./my-ollama-app -o llm.daedalus --gpu rocm
```

- `--gpu auto` probes the host: `/dev/kfd` → ROCm, `/proc/driver/nvidia/gpus`
  → NVIDIA. If nothing is found it warns and builds a CPU-only binary — the
  same binary still runs on GPU-less machines.
- `--gpu none` (the default) builds a CPU-only binary.
- The backend is embedded in the metadata (`daedalus inspect` shows `GPU:`).

At runtime the launcher:

- **without sandbox** (`--isolation none`): pins the matching visibility vars
  (`CUDA_VISIBLE_DEVICES=0`, `NVIDIA_VISIBLE_DEVICES=all`, or
  `HIP_VISIBLE_DEVICES=0`/`ROCR_VISIBLE_DEVICES=0`), which it never
  overwrites if you exported them yourself
- **in the sandbox**: bind-mounts the GPU device nodes over regular-file
  overlays into the rootfs — NVIDIA `/dev/nvidia*` + `/dev/nvidia-caps/*`,
  ROCm `/dev/kfd` + `/dev/dri/renderD*`. Missing nodes are skipped, so the
  binary degrades to CPU gracefully.

To open a GPU device your user must be able to access it on the host. Factories
typically ship root-only (e.g. `0660 root:render` for DRI nodes), the same
policy Docker requires solving with `--group-add render`:

```bash
sudo usermod -aG render,video "$USER"   # then re-login
```

After that, Ollama will offload to the GPU inside the daedalus sandbox just
like in a container, with no NVIDIA Container Toolkit required.
