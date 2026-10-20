# AI and edge runtimes

daedalus can package AI and edge workloads, including Ollama models and MCP
tools for agents.

## Gemma via Ollama (on-device AI)

daedalus detects a Gemma or Ollama layout and will pack the files that are
actually in the project. It does not download a model for you.

`examples/offline-health-agri` is a standard-library scaffold plus a
`Modelfile` (`FROM gemma:2b`) with **no** weights. `diagnose` / `crop` print
hardcoded notes, then POST to `localhost:11434` if Ollama is already up.

For a real GGUF in-tree, use `examples/local-model-kit`:

```bash
./examples/local-model-kit/fetch_weights.sh   # ~241 MB from Hugging Face
daedalus build ./examples/local-model-kit -o local-model-kit.de --isolation none
daedalus inspect local-model-kit.de           # size should be hundreds of MB
```

Host smoke (Ollama **≥ 0.40.5** for Gemma 3): `python3 app.py ask "..."`.
Weights are gitignored; CI skips inference without the file. Without the
GGUF at build time the `.de` is not an offline LLM. There is no 17 MB Gemma
artifact.

```bash
daedalus build ./examples/offline-health-agri -o clinic-agent.daedalus
./clinic-agent.daedalus diagnose
```

## Ollama

Package an app that already contains an Ollama binary and model files:

```bash
daedalus build ./my-ollama-app -o my-ollama-app.daedalus
```

daedalus detects:

- `ollama` in `package.json` scripts or dependencies
- `Modelfile` or `models/*.gguf`

Those files are copied into the payload when they are present on disk. The
builder does not fetch Ollama or a model on its own. At runtime the entrypoint
is whatever the app declared (`ollama serve` when that is the detected
runtime). GPU passthrough below only matters if that binary is actually in
the rootfs.

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
  → NVIDIA. If nothing is found it warns and builds a CPU-only binary - the
  same binary still runs on GPU-less machines.
- `--gpu none` (the default) builds a CPU-only binary.
- The backend is embedded in the metadata (`daedalus inspect` shows `GPU:`).

At runtime the launcher:

- **without sandbox** (`--isolation none`): pins the matching visibility vars
  (`CUDA_VISIBLE_DEVICES=0`, `NVIDIA_VISIBLE_DEVICES=all`, or
  `HIP_VISIBLE_DEVICES=0`/`ROCR_VISIBLE_DEVICES=0`), which it never
  overwrites if you exported them yourself
- **in the sandbox**: bind-mounts the GPU device nodes over regular-file
  overlays into the rootfs - NVIDIA `/dev/nvidia*` + `/dev/nvidia-caps/*`,
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
