#!/usr/bin/env bash
# benchmarks/comparison/aggregate.sh
#
# Merges the results of every measured machine into a single cross-machine
# comparison. Run `run.sh` on several machines (MACHINE=label), then run this
# to produce:
#   benchmarks/comparison/comparison.md   (per-machine table)
#   benchmarks/comparison/machines.md     (machine profiles side by side)

set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RESULTS_DIR="$SCRIPT_DIR/results"
OUT_MD="$SCRIPT_DIR/comparison.md"
OUT_MACHINES="$SCRIPT_DIR/machines.md"

[ -d "$RESULTS_DIR" ] || { echo "no results yet — run run.sh first" >&2; exit 1; }

DIRS=()
for d in "$RESULTS_DIR"/*/; do
    [ -f "$d/results.tsv" ] && DIRS+=("$d")
done
[ "${#DIRS[@]}" -gt 0 ] || { echo "no machine results found" >&2; exit 1; }

get() { # get <k=v lines> <key>
    local v
    v=$(echo "$1" | tr ';' '\n' | sed -n "s/.*$2=//p" | head -1)
    [ -n "$v" ] && echo "$v" || echo "n/a"
}

{
    echo "# Comparative Benchmark — x.bin vs Docker / pkg / AppImage / Flatpak"
    echo
    echo "_Aggregated: $(date -u +%Y-%m-%dT%H:%MZ)_"
    echo
    echo "> Each row is one (machine, packager) measurement. Full methodology and"
    echo "> per-machine details: see \`results/<machine>/comparison.md\`."
    echo
    echo "## Test machines"
    echo
    echo "| Machine | CPU | Cores | RAM | Disk | Root | Env |"
    echo "|---------|-----|-------|-----|------|------|-----|"
    for d in "${DIRS[@]}"; do
        m=$(basename "$d")
        p="$d/profile.txt"
        cpu=$(sed -n 's/^cpu_model: *//p' "$p" | cut -c1-40)
        cores=$(sed -n 's/^cores: *//p' "$p")
        ram=$(sed -n 's/^ram_total: *//p' "$p")
        disk=$(sed -n 's/^disk_total: *//p' "$p")
        root=$(sed -n 's/^root_fs: *//p' "$p")
        env=$(sed -n 's/^env: *//p' "$p")
        echo "| $m | $cpu | $cores | $ram | $disk | $root | $env |"
    done
    echo
    echo "## Results"
    echo
    echo "| Machine | Packager | Artifact | On-disk | Cold start | Warm start | Idle RSS | Host deps |"
    echo "|---------|----------|----------|---------|------------|------------|----------|-----------|"
    for d in "${DIRS[@]}"; do
        m=$(basename "$d")
        while IFS=$'\t' read -r packager rest; do
            a=$(get "$rest" artifact_mib)
            b=$(get "$rest" footprint_mib)
            c=$(get "$rest" cold_ms)
            e=$(get "$rest" warm_ms)
            f=$(get "$rest" rss_mib)
            g=$(get "$rest" host_deps)
            echo "| $m | $packager | $a MiB | $b MiB | $c ms | $e ms | $f MiB | $g |"
        done < "$d/results.tsv"
    done
    echo
    echo "Raw data: \`results/<machine>/results.tsv\`. Re-run: \`bash benchmarks/comparison/run.sh\`."
} > "$OUT_MD"

{
    echo "# Test machines — profiles"
    echo
    for d in "${DIRS[@]}"; do
        echo "## $(basename "$d")"
        echo
        echo '```'
        cat "$d/profile.txt"
        echo '```'
        echo
    done
} > "$OUT_MACHINES"

echo "wrote $OUT_MD"
echo "wrote $OUT_MACHINES"
