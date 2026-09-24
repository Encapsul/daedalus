# The 60-second demo

> **daedalus makes an app consumable in one gesture: `./app.de`. The user sees the artifact, not the packaging. No install, no expertise.**

A live demo, start to finish, in under one minute. The star is an **offline AI agent** (Gemma, air-gapped, no GPU, no cloud) — the hardest thing to deploy in our industry, delivered as one file.

## The script

```bash
# 1. Install daedalus — one command, ~10 s
curl -fsSL https://raw.githubusercontent.com/Encapsul/daedalus/main/install.sh | sh

# 2. Package the app — ~6 s
daedalus build ./examples/offline-health-agri -o clinic-agent.de

# 3. Run it — it's just the file
./clinic-agent.de diagnose
```

Stop there. On screen (worst case):

| Step | Wall clock |
|------|-----------|
| `install.sh` | ~8 s |
| `daedalus build` → 16.9 MB artifact | ~6.2 s |
| cold first run (`diagnose`) | ~1.5 s |
| warm run (`crop`) | ~0.15 s |

## What just happened (the talk track)

- **`clinic-agent.de` is self-extracting.** The stub launcher carries the app,
  the Python stdlib, and the Gemma model recipe in one executable. The payload
  is integrity-checked with SHA-256 before anything runs, and the "app" part
  can be signed with Ed25519 (`daedalus sign`, default-on dev key).
- **It runs offline.** `diagnose` / `crop` use the standard library only and
  degrade gracefully when Ollama is absent — exactly the rural-clinic and
  agricultural-cooperative use case the example targets.
- **Updates don't reship the file.** `daedalus` content-defined chunking only
  transmits changed model chunks (measured ~90% bandwidth saved on a model
  update) — see [SISR](./incremental-updates.md).
- **It runs on a bare machine.** No Python, no `pip install`, no GPU, no codecs.

## One-liner canned summary

> "I took an offline AI assistant that normally needs an internet connection,
> a GPU, and a five-page setup guide, and shipped it as a single 17 MB file
> you can put on a USB stick. That file also updates itself in ~1/10th of the
> download size. And trust is built in: it verifies its own payload before
> starting, and `daedalus verify clinic-agent.de` proves who signed it."

## Replay

```bash
# swap the app, keep daedalus
daedalus build ./examples/hello-web -o hello-web.de
./hello-web.de &        # python stdlib HTTP, no deps
curl -s http://127.0.0.1:8080 | grep Hello

daedalus inspect clinic-agent.de   # show layers + integrity
daedalus scan examples/offline-health-agri   # show detected runtime
```

## When the room is quiet

The whole point of packaging as a single self-verifying artifact is
**adoption**: the user executes one file. That is the difference between "let
me install Python, create a venv, pip install..." and a movie playing for the
person who just double-clicked it. See [Positioning](../concepts/positioning.md).