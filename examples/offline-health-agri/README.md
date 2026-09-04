# Offline Medical/Agricultural Demo with Google Gemma

This demo showcases a truly offline AI assistant for rural clinics and agricultural cooperatives in Africa. No Internet connection, GPU, or cloud services are required at runtime.

## The Real Problem

In rural clinics and agricultural cooperatives, Internet is rare, slow, or unavailable. Data must remain on-site (privacy, sovereignty). An AI assistant that depends on the cloud is useless where help is needed: this is the impact zone of this demo -- reliable advice on a modest offline device.

## What's Included

- **Modelfile**: `FROM gemma:2b` -- the signal that triggers daedalus' automatic `gemma` runtime detection.
- **models/gemma-2b-it-q4.gguf**: Model weights location (placeholder -- 4-byte dummy file).
- **app.py**: Two modes, Python stdlib only (build works without network).
  - `python3 app.py diagnose` -- rural clinic symptom check.
  - `python3 app.py crop` -- agricultural advice + a condition.
  Each mode displays hardcoded support then shows how it would query Gemma via the local Ollama API. If Ollama is unreachable, it gracefully falls back to "offline demo mode" and remains useful.

## Building

From the workspace root, build the CLI then package the demo:

```bash
cargo build --release
daedalus build ./examples/offline-health-agri -o clinic-agent.de
```

## Model Update (SISR Delta)

When Google publishes a new model version (fine-tune, re-quantization), daedalus does not retransmit the entire package: its content-defined chunking (CD) only returns the modified weight chunks. Reproducible benchmark (included `sisr_stage` test):

```
Gemma update (simulated 200 MiB (10% perturbed)):
  20.6 MiB delta vs 200.0 MiB full -- 89.7% bandwidth saved
  (195 changed chunks, 1511 reused)
```

For an actual v1/v2 weight pair, point to the variables and re-run the bench:

```bash
DAEDALUS_SISR_MODEL_V1=v1.gguf DAEDALUS_SISR_MODEL_V2=v2.gguf \
  cargo test -p daedalus-core --release gemma_weight_delta_bandwidth \
  -- --ignored --nocapture
```

In rural areas where bandwidth costs more than computation, reducing transferred bytes by ~9x per update is the most concrete maintenance lever.

## Local Execution with Ollama

Start the Gemma service, then launch the standalone binary:

```bash
ollama serve
ollama run gemma:2b

./clinic-agent.de diagnose
./clinic-agent.de crop
```

With a loaded model, the agent responds via the local API (`POST /api/generate`, host overloadable via `OLLAMA_HOST`).

## Offline Degradation

Without Ollama running, the binary produces useful output: it displays the checks and the exact question the model would have received, in offline demo mode. It never crashes.

## Why Google Should Care

- **Offline first**: useful AI where coverage is lacking, not the reverse.
- **Data sovereignty**: nothing leaves the device.
- **Gemma where the cloud can't reach**: an efficient small model on modest hardware changes the game for African clinics and cooperatives.
