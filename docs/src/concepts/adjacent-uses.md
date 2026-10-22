# Adjacent uses

daedalus is a self-extracting Linux ELF with integrity checks, optional
signatures, and optional SISR deltas. That shape fits more than "ship a
web app". These uses are real when the matching feature is in the build;
they are not marketing claims about models or sandboxes the file does not
contain.

## What unlocks each use

| Domain | Why a single `.de` helps | What you must actually ship |
|--------|--------------------------|-----------------------------|
| USB / courier delivery | One file + one `.pub` | Signed build; recipient runs `trust` then `verify` |
| Air-gapped ops | No registry pull at runtime | `daedalus media stage` → `media verify` → USB `manifest`+`chunks/`; recipient trusts `.pub` then `DAEDALUS_SISR_MANIFEST=…` |
| Classroom / lab | Same runtime on every laptop | Embedded interpreter; no student `pip install` |
| Incident response | Tools run where outbound net is blocked | Signed kit; private key never on the USB |
| Edge fleet | Same packaging for x64/arm builds | Per-arch artifacts (or `--universal` slices) |
| CI provenance | Prove what bytes left the pipeline | `daedalus attest` + offline `--verify` |
| Local AI | App + weights in one file | Real `.gguf` (or equivalent) in the payload; Ollama new enough to load it |

## What we refuse to claim

- That `daedalus verify` on the **build** machine proves origin to a stranger
  (the default dev key is auto-trusted there).
- That seccomp is an application sandbox (it is a denylist of dangerous
  syscalls).
- That Gemma / Ollama is offline if the weights were never packed.

## Demo in-repo

`examples/field-kit` is a stdlib CLI with modes `handoff`, `inventory`,
`classroom`, `incident`, `update` (air-gap release-train card), and `domains`.
CI and the chaos suite assert on those modes. The documented publisher→USB
flow is [Air-gapped deployment](../guides/airgapped.md) (`daedalus media stage`).
For a vendored third-party dependency path, use `examples/bottle-web`.
