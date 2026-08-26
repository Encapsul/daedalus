#!/usr/bin/env bash
# Local build helper - builds stub + CLI and runs tests
# Usage: ./scripts/build.sh

set -euo pipefail

export CARGO_TARGET_DIR="/tmp/daedalus-stub-target"
export RUSTFLAGS="-D warnings"

echo "=== Building stub (musl) ==="
make stub

echo "=== Building CLI ==="
cargo build --release -p daedalus-cli

echo "=== Running tests ==="
cargo test --workspace

echo "=== Smoke test ==="
/tmp/daedalus-stub-target/release/daedalus build examples/hello-web -o /tmp/hello-web.daedalus
/tmp/hello-web.daedalus

echo "=== All checks passed ==="
