#!/usr/bin/env bash
# Local build helper - builds stub + CLI and runs tests
# Usage: ./scripts/build.sh

set -euo pipefail

export CARGO_TARGET_DIR="/tmp/xbin-stub-target"
export RUSTFLAGS="-D warnings"

echo "=== Building stub (musl) ==="
make stub

echo "=== Building CLI ==="
cargo build --release -p xbin-cli

echo "=== Running tests ==="
cargo test --workspace

echo "=== Smoke test ==="
/tmp/xbin-stub-target/release/xbin build examples/hello-web -o /tmp/hello-web.xbin
/tmp/hello-web.xbin

echo "=== All checks passed ==="