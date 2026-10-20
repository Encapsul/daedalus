# Local model kit (real weights)

This example packages a **real** Gemma 3 270M instruction-tuned GGUF when the
file is present on disk. The weights are **not** in git (`*.gguf` is
gitignored). Without them, the kit is just a script - do not call that
"offline AI".

| Item | Value |
|------|--------|
| HF repo | [unsloth/gemma-3-270m-it-GGUF](https://huggingface.co/unsloth/gemma-3-270m-it-GGUF) |
| File | `gemma-3-270m-it-Q4_K_M.gguf` (~241–253 MB) |
| Local path | `models/gemma-3-270m-it-Q4_K_M.gguf` |
| Ollama | **≥ 0.40.5** (Gemma 3); older clients reject the arch |

## Fetch weights

```bash
./examples/local-model-kit/fetch_weights.sh
# or: curl -fL -o examples/local-model-kit/models/gemma-3-270m-it-Q4_K_M.gguf \
#   https://huggingface.co/unsloth/gemma-3-270m-it-GGUF/resolve/main/gemma-3-270m-it-Q4_K_M.gguf
```

## Host smoke (macOS / Linux with Ollama)

```bash
ollama serve   # separate terminal, client 0.40.5+
cd examples/local-model-kit
python3 app.py probe
python3 app.py ask "Say hello in one short sentence."
```

`ask` runs `ollama create` from the local `Modelfile` (FROM the GGUF path) and
`POST /api/generate`. No Hugging Face call at runtime.

## Package with daedalus

Weights must exist **before** `daedalus build` or they will not be in the ELF:

```bash
./examples/local-model-kit/fetch_weights.sh
daedalus build ./examples/local-model-kit -o local-model-kit.de --isolation none
daedalus inspect local-model-kit.de
# Artifact size should be hundreds of MB if the GGUF was embedded.
```

The Linux stub runs the ELF. On macOS you can still **build/inspect** when a
stub is available; **exec** of the `.de` needs Linux (or WSL).

## CI vs local

| Check | CI | Local with weights |
|-------|----|--------------------|
| `app.py probe` without GGUF | exit 2 (expected) | - |
| Unit/integration skip | yes if no GGUF | - |
| `ask` / packaged inference | **not** in default CI | run manually |

## Honesty

- Clinic (`examples/offline-health-agri`) remains a scaffold **without** weights.
- This kit is the niche wedge only when `models/*.gguf` is present at build.
- `daedalus` does not download models for you.
