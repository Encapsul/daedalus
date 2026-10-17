# Clinic scaffold (not a packaged model)

Python standard library only, plus a `Modelfile` (`FROM gemma:2b`). There is
no GGUF in this directory. `diagnose` and `crop` print hardcoded notes, then
POST to a local Ollama if one is already listening. If it is not, they print
the prompt. This is not medical advice and it is not the project demo. The
demo is `examples/hello-web`.

## Building

```bash
daedalus build ./examples/offline-health-agri -o clinic-agent.daedalus
./clinic-agent.daedalus diagnose
```

The artifact is about the size of the script and the embedded interpreter, not
the size of Gemma.

## Synthetic delta

A unit test perturbs 10% of a 200 MiB buffer and reports 20.6 MiB of delta
(89.7% fewer bytes than sending the whole buffer). That is not a pair of
model weights. For a real pair:

```bash
DAEDALUS_SISR_MODEL_V1=v1.gguf DAEDALUS_SISR_MODEL_V2=v2.gguf \
  cargo test -p daedalus-core --release gemma_weight_delta_bandwidth \
  -- --ignored --nocapture
```

## Local Ollama, if you already have one

Start Ollama yourself if you want a model answer. This script does not start it
and does not contain weights.

```bash
ollama serve
ollama run gemma:2b

./clinic-agent.daedalus diagnose
./clinic-agent.daedalus crop
```

The app talks to `POST /api/generate` (`OLLAMA_HOST`, default
`http://localhost:11434`). It does not start Ollama and it does not carry
the weights.

## Offline path

Without Ollama, the script prints the checks and the prompt. It does not crash.
