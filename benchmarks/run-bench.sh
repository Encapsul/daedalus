#!/usr/bin/env bash
# benchmarks/run-bench.sh — Build an app with xbin and record machine specs + memory peak.
#
# Usage:
#   ./benchmarks/run-bench.sh <app-dir> [output-name]
#
# Example:
#   ./benchmarks/run-bench.sh repo/uptime-kuma uptime-kuma
#
# Output: benchmarks/<output-name>-<date>.md

set -euo pipefail

APP_DIR="${1:?Usage: run-bench.sh <app-dir> [output-name]}"
APP_NAME="${2:-$(basename "$APP_DIR")}"
BENCH_DIR="$(cd "$(dirname "$0")" && pwd)"
TIMESTAMP="$(date +%Y%m%d-%H%M%S)"
OUT_FILE="${BENCH_DIR}/${APP_NAME}-${TIMESTAMP}.md"
XBIN="${XBIN:-$HOME/.local/bin/xbin}"

# ── Machine specs ──────────────────────────────────────────────────────
CPU_MODEL="$(lscpu | grep 'Model name' | sed 's/Model name:\s*//')"
CPU_CORES="$(nproc)"
CPU_THREADS="$(lscpu | grep '^CPU(s):' | awk '{print $2}')"
ARCH="$(uname -m)"
RAM_TOTAL_KB="$(awk '/^MemTotal:/ {print $2}' /proc/meminfo)"
RAM_TOTAL_GB=$(awk "BEGIN {printf \"%.1f\", ${RAM_TOTAL_KB}/1024/1024}")
SWAP_TOTAL_KB="$(awk '/^SwapTotal:/ {print $2}' /proc/meminfo)"
SWAP_TOTAL_GB=$(awk "BEGIN {printf \"%.1f\", ${SWAP_TOTAL_KB}/1024/1024}")

DISK_TOTAL="$(df -h /tmp | awk 'NR==2{print $2}')"
DISK_USED="$(df -h /tmp | awk 'NR==2{print $3}')"
DISK_AVAIL="$(df -h /tmp | awk 'NR==2{print $4}')"
DISK_FS="$(df -T /tmp | awk 'NR==2{print $1}')"

KERNEL="$(uname -r)"
OS="$(. /etc/os-release 2>/dev/null && echo "$PRETTY_NAME" || uname -s)"

# Detect if /tmp is tmpfs (live USB, RAM-backed)
TMPFS_MODE="$(mount | grep 'on /tmp ' | grep -o 'tmpfs' || echo "no")"

# ── Disk I/O benchmark (sequential write/read 64MB) ───────────────────
IO_TEST_FILE="/tmp/.xbin-bench-io"
dd if=/dev/zero of="$IO_TEST_FILE" bs=1M count=64 oflag=direct 2>/dev/null
IO_WRITE_MS=$(date +%s%3N)
sync
IO_WRITE_END=$(date +%s%3N)
IO_WRITE_TIME=$((IO_WRITE_END - IO_WRITE_MS))

IO_READ_MS=$(date +%s%3N)
dd if="$IO_TEST_FILE" of=/dev/null bs=1M iflag=direct 2>/dev/null
IO_READ_END=$(date +%s%3N)
IO_READ_TIME=$((IO_READ_END - IO_READ_MS))
rm -f "$IO_TEST_FILE"

# ── Build with memory monitoring ───────────────────────────────────────
MEM_LOG=$(mktemp)
{
    while true; do
        rss=0
        while IFS= read -r pid; do
            rss=$((rss + $(awk '/^VmRSS:/ {print $2}' /proc/$pid/status 2>/dev/null || echo 0)))
        done < <(pgrep -f "xbin build" 2>/dev/null || true)
        echo "$(date +%s) $rss" >> "$MEM_LOG"
        sleep 0.1
    done
} &
MONITOR_PID=$!
trap "kill $MONITOR_PID 2>/dev/null; rm -f $MEM_LOG" EXIT

BUILD_START=$(date +%s%3N)
BUILD_STDERR=$(mktemp)

set +e
"$XBIN" build "$APP_DIR" -o "/tmp/${APP_NAME}-bench.xbin" --no-install 2>"$BUILD_STDERR"
BUILD_EXIT=$?
set -e

BUILD_END=$(date +%s%3N)
BUILD_MS=$((BUILD_END - BUILD_START))

sleep 1
kill "$MONITOR_PID" 2>/dev/null || true
wait "$MONITOR_PID" 2>/dev/null || true

# ── Parse results ──────────────────────────────────────────────────────
BUILD_LOG=$(cat "$BUILD_STDERR" 2>/dev/null || echo "")
rm -f "$BUILD_STDERR"

OUTPUT_SIZE=$(stat -c%s "/tmp/${APP_NAME}-bench.xbin" 2>/dev/null || echo 0)
OUTPUT_SIZE_MB=$(awk "BEGIN {printf \"%.1f\", ${OUTPUT_SIZE}/1024/1024}")

# Peak memory (KB)
PEAK_MEM_KB=0
LAST_NONZERO_KB=0
while read -r _ts rss; do
    if [ "$rss" -gt "$PEAK_MEM_KB" ] 2>/dev/null; then
        PEAK_MEM_KB=$rss
    fi
    if [ "$rss" -gt 0 ] 2>/dev/null; then
        LAST_NONZERO_KB=$rss
    fi
done < "$MEM_LOG"
# Use the last nonzero value if peak is 0 (process already exited)
[ "$PEAK_MEM_KB" -eq 0 ] && PEAK_MEM_KB=$LAST_NONZERO_KB
PEAK_MEM_MB=$(awk "BEGIN {printf \"%.1f\", ${PEAK_MEM_KB}/1024}")

# Min memory (excluding zeros)
MIN_MEM_KB=""
while read -r _ts rss; do
    if [ "$rss" -gt 0 ] 2>/dev/null; then
        if [ -z "$MIN_MEM_KB" ] || [ "$rss" -lt "$MIN_MEM_KB" ]; then
            MIN_MEM_KB=$rss
        fi
    fi
done < "$MEM_LOG"
MIN_MEM_MB=$(awk "BEGIN {printf \"%.1f\", ${MIN_MEM_KB:-0}/1024}")

# Build time formatted
if [ "$BUILD_MS" -ge 1000 ]; then
    BUILD_TIME="$(awk "BEGIN {printf \"%.1f\", ${BUILD_MS}/1000}")s"
else
    BUILD_TIME="${BUILD_MS}ms"
fi

XBIN_VER=$("$XBIN" --version 2>/dev/null | head -1 | sed 's/^xbin //' || echo "unknown")

# ── Estimate: would this build survive on 8GB tmpfs live USB? ─────────
# On tmpfs live USB, /tmp + swap ≈ RAM. xbin needs: source + rootfs + payload + tar
# Estimate: peak RSS × 2 (build uses temp files in /tmp)
TMPFS_NOTE="N/A (not tmpfs)"
if [ "$TMPFS_MODE" = "tmpfs" ]; then
    TMPFS_NOTE="/tmp is tmpfs (RAM-backed)"
fi

# ── Generate report ────────────────────────────────────────────────────
cat > "$OUT_FILE" <<EOF
# Benchmark: ${APP_NAME}

- **Date**: $(date -Iseconds)
- **Tool**: xbin ${XBIN_VER}

## Machine

| Spec | Value |
|------|-------|
| OS | ${OS} |
| Kernel | ${KERNEL} |
| CPU | ${CPU_MODEL} |
| Cores / Threads | ${CPU_CORES} / ${CPU_THREADS} |
| Architecture | ${ARCH} |
| RAM | ${RAM_TOTAL_GB} GB |
| Swap | ${SWAP_TOTAL_GB} GB |
| Disk (${DISK_FS}) | ${DISK_TOTAL} total, ${DISK_AVAIL} free |
| /tmp type | ${TMPFS_NOTE} |

## Disk I/O

| Operation | Time |
|-----------|------|
| Write 64MB | ${IO_WRITE_TIME}ms |
| Read 64MB | ${IO_READ_TIME}ms |

## Build result

| Metric | Value |
|--------|-------|
| App | ${APP_NAME} |
| Source | ${APP_DIR} |
| Output | /tmp/${APP_NAME}-bench.xbin |
| Output size | ${OUTPUT_SIZE_MB} MB |
| Build time | ${BUILD_TIME} |
| Peak RSS | ${PEAK_MEM_MB} MB |
| Min RSS (active) | ${MIN_MEM_MB} MB |
| Exit code | ${BUILD_EXIT} |

## 8GB tmpfs live USB estimate

| Constraint | Value | Safe? |
|------------|-------|-------|
| RAM | 8 GB | - |
| Peak RSS | ${PEAK_MEM_MB} MB | $([ $(echo "$PEAK_MEM_MB < 8192" | bc -l) -eq 1 ] && echo "YES" || echo "NO") |
| Peak RSS + tmpfs overhead (~2×) | $(awk "BEGIN {printf \"%.0f\", ${PEAK_MEM_KB}*2/1024}") MB | $([ $(echo "$PEAK_MEM_KB*2/1024 < 8192" | bc -l) -eq 1 ] && echo "YES" || echo "NO") |

> **Note**: On a live USB with 8GB RAM, /tmp is tmpfs and consumes RAM.
> xbin writes temp files (tar, compressed payload) in /tmp during build.
> Peak RSS + tmpfs usage must fit within available RAM + swap.

## Build log

\`\`\`
${BUILD_LOG}
\`\`\`

## Memory timeline

\`\`\`
time_s,rss_kb
$(cat "$MEM_LOG" 2>/dev/null || echo "no data")
\`\`\`
EOF

echo "Benchmark saved to: $OUT_FILE"
