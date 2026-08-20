#!/usr/bin/env bash
# Download all GGUF quantizations from PleIAs/Baguettotron-GGUF
# Usage: ./download_models.sh [quantizations...]
#   e.g. ./download_models.sh Q4_K_S Q4_0 F16
#   (no args = download all)
set -euo pipefail

DEPS_DIR="$(dirname "$0")/.deps"
mkdir -p "$DEPS_DIR"

ALL_QUANTS="IQ4_XS Q4_0 Q4_K_S Q4_K_M Q5_K_S Q5_K_M Q6_K Q8_0 F16"
QUANTS="${*:-$ALL_QUANTS}"

for q in $QUANTS; do
    echo "Downloading Baguettotron-${q}.gguf..."
    hf download PleIAs/Baguettotron-GGUF "Baguettotron-${q}.gguf" \
        --local-dir "$DEPS_DIR"
done

echo "Downloaded $(ls -1 "$DEPS_DIR"/Baguettotron-*.gguf 2>/dev/null | wc -l) models ($(du -sh "$DEPS_DIR" | cut -f1))"
