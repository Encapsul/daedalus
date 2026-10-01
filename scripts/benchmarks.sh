#!/usr/bin/env bash
# benchmarks.sh - reproducible measurements for published claims
#
# Measures what can be measured on the build host. Low-bandwidth and
# real-device numbers must come from Raspberry Pi runs (see docs/), not here:
# this host is not representative of the edge deployment target.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
DAEDALUS="${DAEDALUS:-$REPO_ROOT/target/release/daedalus}"
OUT="${OUT:-/tmp/daedalus-bench-$$}"
mkdir -p "$OUT"

file_size_mb() { python3 -c "import os;print(f'{os.path.getsize(\"$1\")/1024/1024:.2f}')"; }

# Time from exec to the app printing its first line of output, capped so a
# long-running server does not hang the harness.
time_to_first_output_ms() {
  local bin="$1" limit_ms="${2:-20000}"
  python3 - "$bin" "$limit_ms" <<'PY'
import subprocess, sys, time
binary, limit = sys.argv[1], int(sys.argv[2]) / 1000
t0 = time.perf_counter()
try:
    p = subprocess.Popen([binary], stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                         text=True, errors="replace")
    line = p.stdout.readline()
    dt = (time.perf_counter() - t0) * 1000
    print(int(dt))
except Exception:
    print(-1)
finally:
    try:
        p.kill()
    except Exception:
        pass
PY
}

echo "# Daedalus benchmarks"
echo "date:       $(date -Iseconds)"
echo "machine:    $(uname -sm)"
echo "daedalus:   $("$DAEDALUS" --version 2>/dev/null | head -1)"
echo ""
echo "## build size + time to first output (3 runs, host-local)"
echo ""
printf "%-22s %10s %14s\n" "example" "size_MB" "first_out_ms"

for example in hello-web hello-api; do
  app="$REPO_ROOT/examples/$example"
  [[ -d "$app" ]] || continue
  bin="$OUT/$example.de"
  if ! "$DAEDALUS" build "$app" -o "$bin" >"$OUT/$example.build.log" 2>&1; then
    printf "%-22s %10s %14s\n" "$example" "BUILD FAILED" "-"
    continue
  fi
  total=0
  runs=0
  for _ in 1 2 3; do
    ms=$(time_to_first_output_ms "$bin")
    if [[ "$ms" =~ ^[0-9]+$ ]]; then total=$((total+ms)); runs=$((runs+1)); fi
  done
  if [[ $runs -gt 0 ]]; then
    avg=$((total/runs))
  else
    avg=-1
  fi
  printf "%-22s %10s %14s\n" "$example" "$(file_size_mb "$bin")" "$avg"
done

echo ""
echo "## methodology"
echo "- size_MB: on-disk size of the built .de"
echo "- first_out_ms: wall clock from exec(2) to the first line the app writes"
echo "- 3 runs averaged; the cache is warm after the first run, so this is NOT a"
echo "  cold-disk figure"
echo "- low-bandwidth delta-update numbers require a real device (Raspberry Pi)"
echo "  and are not measured here"
echo ""
echo "artifacts: $OUT"
