#!/usr/bin/env bash
# Local build helper - builds stub + CLI and runs tests
# Usage: ./scripts/build.sh

set -euo pipefail

export CARGO_TARGET_DIR="/tmp/erebus-stub-target"
export RUSTFLAGS="-D warnings"

echo "=== Building stub (musl) ==="
make stub

echo "=== Building CLI ==="
cargo build --release -p erebus-cli

echo "=== Running tests ==="
cargo test --workspace

echo "=== Smoke test ==="
/tmp/erebus-stub-target/release/erebus build examples/hello-web -o /tmp/hello-web.ere
/tmp/hello-web.ere

echo "=== All checks passed ==="
