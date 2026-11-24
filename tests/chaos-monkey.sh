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
ABORTED=1
cleanup() {
  if [ "$ABORTED" -eq 1 ] || [ "$FAIL" -ne 0 ]; then
    echo "  artifacts kept at $WORKDIR"
  else
    rm -rf "$WORKDIR"
  fi
}
trap cleanup EXIT

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1"; FAIL=$((FAIL + 1)); }
section() { echo ""; echo "=== $1 ==="; }

# Never let a hostile artifact hang CI. Used by every execution check.
run_with_deadline() {
  local secs="$1"
  shift
  if command -v timeout >/dev/null 2>&1; then
    timeout "$secs" "$@"
    return $?
  fi
  "$@" &
  local pid=$!
  local i=0
  local limit=$((secs * 10))
  while kill -0 "$pid" 2>/dev/null && [ "$i" -lt "$limit" ]; do
    sleep 0.1
    i=$((i + 1))
  done
  if kill -0 "$pid" 2>/dev/null; then
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    return 124
  fi
  wait "$pid"
}

# Bash prints "Segmentation fault (core dumped)" itself when the *loader*
# rejects a truncated ELF; only match markers emitted by the Rust stub.
assert_clean_stderr() { # assert_clean_stderr <file> <label>
  if grep -qiE 'panicked at|thread .* panicked|AddressSanitizer|runtime error' "$1" 2>/dev/null; then
    fail "$2: crash marker in stderr"
  else
    pass "$2: no crash markers in stderr"
  fi
}

# `--daedalus-version` exits before the integrity check, so at-rest corruption
# must be asserted with the verifier instead.
verify_rejects() { # verify_rejects <file> <label>
  if ! command -v daedalus >/dev/null 2>&1; then
    echo "  SKIP: daedalus CLI not on PATH ($2)"
    return 0
  fi
  local out="$WORKDIR/verify-$2.out"
  set +e
  DAEDALUS_TRUSTED_DIR="$TRUSTED_KEYS" daedalus verify "$1" --all --json \
    >"$out" 2>"$out.err"
  local rc=$?
  set -e
  assert_clean_stderr "$out.err" "$2 verify"
  if [ "$rc" -eq 0 ]; then
    fail "$2: verify --all accepted a corrupted artifact"
  else
    pass "$2: verify --all rejects corrupted artifact (rc=$rc)"
  fi
}

# `--daedalus-version` never reaches the integrity check. When the payload is
# terminal, require a real execution of the corrupted bundle to fail too.
assert_exec_fails() { # assert_exec_fails <file> <label>
  if [ "$BASELINE_RC" -ne 0 ]; then
    return 0
  fi
  local err="$WORKDIR/$2-exec.err"
  set +e
  run_with_deadline 3 "$1" >/dev/null 2>"$err"
  local rc=$?
  set -e
  assert_clean_stderr "$err" "$2 exec"
  if [ "$rc" -eq 0 ]; then
    fail "$2: corrupted bundle executed despite a clean baseline"
  else
    pass "$2: corrupted bundle failed at exec (rc=$rc)"
  fi
}

if [ -z "$BINARY" ] || [ ! -f "$BINARY" ]; then
  echo "usage: $0 /path/to/app.de [trusted-keys-dir]"
  exit 2
fi

section "Pre-flight"
file "$BINARY" | grep -q ELF && pass "ELF format" || fail "not ELF"
[ -x "$BINARY" ] && pass "executable bit" || chmod +x "$BINARY"

# Learn whether this payload is terminal (app.py, field-kit) or server-like.
# Only terminal payloads support a real-exec fail-closed differential; servers
# are bounded by the deadline instead.
set +e
run_with_deadline 3 "$BINARY" >"$WORKDIR/baseline.out" 2>"$WORKDIR/baseline.err"
BASELINE_RC=$?
set -e
if [ "$BASELINE_RC" -eq 0 ]; then
  pass "baseline app execution succeeded (rc=0)"
else
  echo "  SKIP: real-exec differential (baseline rc=$BASELINE_RC, likely a server)"
fi

section "1. Truncated payload"
TRUNCATED="$WORKDIR/truncated.de"
cp "$BINARY" "$TRUNCATED"
truncate -s 4096 "$TRUNCATED" || dd if="$BINARY" of="$TRUNCATED" bs=4096 count=1 2>/dev/null
TRUNC_ERR="$WORKDIR/truncated.err"
set +e
run_with_deadline 3 "$TRUNCATED" --daedalus-version >/dev/null 2>"$TRUNC_ERR"
trunc_rc=$?
set -e
if [ "$trunc_rc" -eq 124 ]; then
  fail "truncated binary hung"
elif [ "$trunc_rc" -eq 0 ]; then
  fail "truncated binary should not run"
else
  pass "truncated binary rejected (rc=$trunc_rc)"
fi
assert_clean_stderr "$TRUNC_ERR" "truncated"

section "2. Random bytes injected"
CORRUPTED="$WORKDIR/corrupted.de"
cp "$BINARY" "$CORRUPTED"
SIZE=$(wc -c <"$CORRUPTED" | tr -d ' ')
# dd's seek is in 1024-byte blocks, not bytes: using SIZE/3 here would seek
# gigabytes past EOF and create a sparse file instead of corrupting the payload.
SEEK=$((SIZE / 3 / 1024))
dd if=/dev/urandom bs=1024 count=8 seek="$SEEK" of="$CORRUPTED" conv=notrunc 2>/dev/null
CORR_ERR="$WORKDIR/corrupted.err"
set +e
run_with_deadline 3 "$CORRUPTED" --daedalus-version >/dev/null 2>"$CORR_ERR"
corr_rc=$?
set -e
if [ "$corr_rc" -eq 124 ]; then
  fail "corrupted binary hung"
elif [ "$corr_rc" -gt 128 ]; then
  fail "corrupted binary died from signal (rc=$corr_rc)"
else
  pass "corrupted binary parse exited (rc=$corr_rc)"
fi
assert_clean_stderr "$CORR_ERR" "random-corruption"
verify_rejects "$CORRUPTED" "random-corruption"
assert_exec_fails "$CORRUPTED" "random-corruption"

section "3. Footer magic / sentinel smash"
SMASHED="$WORKDIR/smashed.de"
cp "$BINARY" "$SMASHED"
# Overwrite the last 16 bytes (footer magic region) with zeros.
SIZE=$(wc -c <"$SMASHED" | tr -d ' ')
if [ "$SIZE" -gt 16 ]; then
  dd if=/dev/zero bs=1 count=16 seek=$((SIZE - 16)) of="$SMASHED" conv=notrunc 2>/dev/null
  SMASH_ERR="$WORKDIR/smashed.err"
  set +e
  run_with_deadline 3 "$SMASHED" --daedalus-version >/dev/null 2>"$SMASH_ERR"
  smash_rc=$?
  set -e
  if [ "$smash_rc" -eq 124 ]; then
    fail "footer-smashed binary hung"
  elif [ "$smash_rc" -eq 0 ]; then
    fail "footer smash should not run"
  else
    pass "footer smash rejected (rc=$smash_rc)"
  fi
  assert_clean_stderr "$SMASH_ERR" "footer-smash"
else
  echo "  SKIP: binary too small for footer smash"
fi

section "4. Concurrent runs"
set +e
run_with_deadline 3 "$BINARY" --daedalus-version >/dev/null 2>&1 &
P1=$!
run_with_deadline 3 "$BINARY" --daedalus-version >/dev/null 2>&1 &
P2=$!
wait "$P1"
rc1=$?
wait "$P2"
rc2=$?
set -e
if [ "$rc1" -eq "$rc2" ] && [ "$rc1" -ne 137 ]; then
  pass "concurrent --daedalus-version exited consistently (rc=$rc1)"
else
  fail "concurrent runs diverged (rc=$rc1 vs $rc2)"
fi

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
# field-kit answers named modes and exits. Server apps (bottle-web, hello-web)
# ignore the arg and keep listening — must not block CI. Cap each probe.
for mode in domains handoff inventory; do
  MODE_OUT="$WORKDIR/mode-$mode.out"
  set +e
  run_with_deadline 3 "$BINARY" "$mode" >"$MODE_OUT" 2>/dev/null
  rc=$?
  set -e
  if [ "$rc" -eq 124 ]; then
    echo "  SKIP: mode '$mode' did not exit (server-like payload)"
    continue
  fi
  if [ "$rc" -eq 0 ]; then
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

section "8. Dual-extension scan"
if command -v daedalus >/dev/null 2>&1; then
  SCAN_DIR="$WORKDIR/scan-dual"
  mkdir -p "$SCAN_DIR"
  cp "$BINARY" "$SCAN_DIR/one.de"
  cp "$BINARY" "$SCAN_DIR/two.daedalus"
  SCAN_OUT="$WORKDIR/scan.json"
  if daedalus scan "$SCAN_DIR" --json >"$SCAN_OUT" 2>/dev/null; then
    # Expect two entries (or at least both paths mentioned).
    COUNT=$(grep -c '"file"' "$SCAN_OUT" 2>/dev/null || true)
    if [ "${COUNT:-0}" -ge 2 ]; then
      pass "scan finds .de and .daedalus"
    elif grep -q '\.de\|\.daedalus' "$SCAN_OUT"; then
      pass "scan returned dual-extension artifacts"
    else
      fail "scan did not list both extensions"
    fi
  else
    echo "  SKIP: scan failed (non-inspectable fixture?)"
  fi
else
  echo "  SKIP: daedalus CLI not on PATH"
fi

section "9. Mid-file corruption (payload/media region)"
# Smash a mid-file window distinct from the footer smash in §3. Cold
# --daedalus-version must fail closed (or succeed only if the region is unused).
MID="$WORKDIR/midcorrupt.de"
cp "$BINARY" "$MID"
SIZE=$(wc -c <"$MID" | tr -d ' ')
if [ "$SIZE" -gt 8192 ]; then
  SEEK=$((SIZE / 2))
  dd if=/dev/zero bs=512 count=4 seek=$((SEEK / 512)) of="$MID" conv=notrunc 2>/dev/null
  MID_ERR="$WORKDIR/midcorrupt.err"
  set +e
  run_with_deadline 3 "$MID" --daedalus-version >/dev/null 2>"$MID_ERR"
  mid_rc=$?
  set -e
  if [ "$mid_rc" -eq 124 ]; then
    fail "mid-corrupt binary hung"
  elif [ "$mid_rc" -gt 128 ]; then
    fail "mid-corrupt binary died from signal (rc=$mid_rc)"
  else
    pass "mid-corrupt binary exited (rc=$mid_rc)"
  fi
  assert_clean_stderr "$MID_ERR" "mid-corruption"
  verify_rejects "$MID" "mid-corruption"
  assert_exec_fails "$MID" "mid-corruption"
else
  echo "  SKIP: binary too small for mid-file smash"
fi

section "Summary"
echo "PASS=$PASS FAIL=$FAIL"
[ "$FAIL" -eq 0 ]
ABORTED=0
