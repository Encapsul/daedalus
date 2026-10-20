#!/usr/bin/env bash
# Fetch the real Gemma 3 270M IT Q4_K_M GGUF into models/ (gitignored).
# Source: https://huggingface.co/unsloth/gemma-3-270m-it-GGUF
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
DEST="$ROOT/models/gemma-3-270m-it-Q4_K_M.gguf"
REPO="${HF_REPO:-unsloth/gemma-3-270m-it-GGUF}"
FILE="${HF_FILE:-gemma-3-270m-it-Q4_K_M.gguf}"

mkdir -p "$ROOT/models"
if [ -f "$DEST" ]; then
  SIZE=$(wc -c <"$DEST" | tr -d ' ')
  echo "already present: $DEST ($SIZE bytes)"
  exit 0
fi

if command -v curl >/dev/null 2>&1; then
  URL="https://huggingface.co/${REPO}/resolve/main/${FILE}"
  echo "downloading $URL"
  curl -fL --retry 3 -o "$DEST.partial" "$URL"
  mv "$DEST.partial" "$DEST"
else
  echo "curl required" >&2
  exit 1
fi

# GGUF magic
python3 - "$DEST" <<'PY'
import sys
from pathlib import Path
p = Path(sys.argv[1])
magic = p.read_bytes()[:4]
if magic != b"GGUF":
    raise SystemExit(f"not a GGUF file: magic={magic!r}")
print(f"ok: {p} ({p.stat().st_size} bytes)")
PY
