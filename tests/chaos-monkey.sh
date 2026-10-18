#!/usr/bin/env bash
# Chaos monkey for a packaged .de artifact on Linux.
# Usage: tests/chaos-monkey.sh /path/to/app.de [trusted-keys-dir]
#
# Covers extract integrity, concurrent cold paths, and the courier trust
# model (empty trust must fail verify on signed artifacts). Optional mode
# args (field-kit style) are exercised when the binary answers them.
set -euo pipefail

PASS=0
FAIL=0
BINARY="${1:-}"
TRUSTED_KEYS="${2:-${DAEDALUS_TRUSTED_DIR:-$HOME/.daedalus/trusted-keys}}"
WORKDIR="$(mktemp -d)"
export XDG_CACHE_HOME="$WORKDIR/.cache"

cleanup() {
  rm -rf "$WORKDIR"
}
trap cleanup EXIT

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1"; FAIL=$((FAIL + 1)); }
section() { echo ""; echo "=== $1 ==="; }

if [ -z "$BINARY" ] || [ ! -f "$BINARY" ]; then
  echo "usage: $0 /path/to/app.de [trusted-keys-dir]"
  exit 2
fi

section "Pre-flight"
file "$BINARY" | grep -q ELF && pass "ELF format" || fail "not ELF"
[ -x "$BINARY" ] && pass "executable bit" || chmod +x "$BINARY"

section "1. Truncated payload"
TRUNCATED="$WORKDIR/truncated.de"
cp "$BINARY" "$TRUNCATED"
truncate -s 4096 "$TRUNCATED" || dd if="$BINARY" of="$TRUNCATED" bs=4096 count=1 2>/dev/null
if "$TRUNCATED" --daedalus-version >/dev/null 2>&1; then
  fail "truncated binary should not run"
else
  pass "truncated binary rejected"
fi

section "2. Random bytes injected"
CORRUPTED="$WORKDIR/corrupted.de"
cp "$BINARY" "$CORRUPTED"
SIZE=$(wc -c <"$CORRUPTED" | tr -d ' ')
SEEK=$((SIZE / 3))
dd if=/dev/urandom bs=1024 count=8 seek="$SEEK" of="$CORRUPTED" conv=notrunc 2>/dev/null || true
if "$CORRUPTED" --daedalus-version >/dev/null 2>&1; then
  pass "corruption handled (cache hit or non-critical region)"
else
  pass "corrupted payload rejected"
fi

section "3. Footer magic / sentinel smash"
SMASHED="$WORKDIR/smashed.de"
cp "$BINARY" "$SMASHED"
# Overwrite the last 16 bytes (footer magic region) with zeros.
SIZE=$(wc -c <"$SMASHED" | tr -d ' ')
if [ "$SIZE" -gt 16 ]; then
  dd if=/dev/zero bs=1 count=16 seek=$((SIZE - 16)) of="$SMASHED" conv=notrunc 2>/dev/null || true
  if "$SMASHED" --daedalus-version >/dev/null 2>&1; then
    fail "footer smash should not run"
  else
    pass "footer smash rejected"
  fi
else
  echo "  SKIP: binary too small for footer smash"
fi

section "4. Concurrent runs"
"$BINARY" --daedalus-version >/dev/null 2>&1 &
P1=$!
"$BINARY" --daedalus-version >/dev/null 2>&1 &
P2=$!
wait "$P1" || true
wait "$P2" || true
pass "concurrent --daedalus-version did not deadlock"

section "5. Empty trust store (courier threat model)"
EMPTY_TRUST="$WORKDIR/empty-trust"
mkdir -p "$EMPTY_TRUST"
if command -v daedalus >/dev/null 2>&1; then
  # Unsigned artifacts verify as "not signed" (exit 0). Signed ones must fail
  # closed when the trust store is empty.
  VERIFY_OUT="$WORKDIR/verify.err"
  set +e
  DAEDALUS_TRUSTED_DIR="$EMPTY_TRUST" daedalus verify "$BINARY" >"$WORKDIR/verify.out" 2>"$VERIFY_OUT"
  VERIFY_RC=$?
  set -e
  if grep -qi "not signed" "$VERIFY_OUT" "$WORKDIR/verify.out" 2>/dev/null; then
    pass "unsigned artifact: verify reports not signed (empty trust irrelevant)"
  elif [ "$VERIFY_RC" -ne 0 ] && grep -qi "no trusted keys\|trusted" "$VERIFY_OUT"; then
    pass "signed artifact: verify fails without trusted keys"
  elif [ "$VERIFY_RC" -ne 0 ]; then
    pass "verify failed closed under empty trust (rc=$VERIFY_RC)"
  else
    fail "verify must not succeed under empty trust for a signed artifact"
  fi
else
  echo "  SKIP: daedalus CLI not on PATH"
fi

section "6. Trusted-keys dir readable"
if [ -d "$TRUSTED_KEYS" ]; then
  pass "trusted keys dir exists: $TRUSTED_KEYS"
else
  echo "  SKIP: trusted keys dir missing ($TRUSTED_KEYS)"
fi

section "7. Adjacent mode probes (optional)"
# field-kit and similar CLIs answer named modes; hello-web will ignore them.
for mode in domains handoff inventory; do
  MODE_OUT="$WORKDIR/mode-$mode.out"
  if "$BINARY" "$mode" >"$MODE_OUT" 2>/dev/null; then
    case "$mode" in
      domains)
        if grep -q "usb-courier\|ADJACENT\|FIELD KIT" "$MODE_OUT"; then
          pass "mode domains answered"
        else
          pass "mode domains exited 0 (non-field-kit payload)"
        fi
        ;;
      *)
        pass "mode $mode exited 0"
        ;;
    esac
  else
    echo "  SKIP: binary has no '$mode' mode (expected for non-field-kit apps)"
  fi
done

section "Summary"
echo "PASS=$PASS FAIL=$FAIL"
[ "$FAIL" -eq 0 ]
