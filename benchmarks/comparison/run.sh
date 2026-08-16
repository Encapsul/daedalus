#!/usr/bin/env bash
# benchmarks/comparison/run.sh
#
# Cross-packager benchmark: packs the same reference app with every packager
# available on the host (x.bin, Docker, pkg, AppImage, Flatpak) and measures:
#
#   artifact size       - size of the single distributable file/image
#   on-disk footprint   - space used at run time (extracted rootfs / image)
#   cold start          - launch to first HTTP 200
#   warm start          - second launch (cache hit), x.bin only
#   idle RSS            - resident set of the serving process
#   host deps           - packages/services the host must provide
#
# Every run records a machine profile (CPU model, RAM, disk, root device,
# live-system indicator, tool versions) so results are comparable across
# machines — see results/<machine>/profile.txt.
#
# Usage: run.sh [APP_DIR]
#   MACHINE=label   machine name for the results dir (default: hostname)
#   results are written to benchmarks/comparison/results/$MACHINE/
#
# Aggregate multiple machines: bash benchmarks/comparison/aggregate.sh
#
# Environment:
#   EREBUS_BIN      path to the erebus CLI (default: /tmp/erebus-stub-target/release/erebus)
#   XBIN_STUB_PATH path to a freshly built stub (default: make stub)
#   PORT_BASE      first port of the measurement range (default: 21300)

set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_DIR="${1:-$SCRIPT_DIR/apps/hello-node}"
MACHINE="${MACHINE:-$(hostname)}"
OUT_DIR="$SCRIPT_DIR/results/$MACHINE"
EREBUS_BIN="${EREBUS_BIN:-/tmp/erebus-stub-target/release/erebus}"
PORT_BASE="${PORT_BASE:-21300}"

WORK="$SCRIPT_DIR/work"
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR" "$WORK"
: > "$OUT_DIR/results.tsv"

log()  { echo "[run.sh] $*"; }
die()  { echo "[run.sh] ERROR: $*" >&2; exit 1; }

require_cmd() { command -v "$1" >/dev/null 2>&1; }

# Ensure a freshly built stub is used (find_stub silently prefers old
# installed stubs otherwise — see xbin-cli/src/commands/build.rs).
if [ -z "${XBIN_STUB_PATH:-}" ]; then
    MUSL_STUB="$REPO_ROOT/target/x86_64-unknown-linux-musl/release/xbin-stub"
    if [ ! -f "$MUSL_STUB" ]; then
        log "building stub (make stub)"
        (cd "$REPO_ROOT" && make stub) >/dev/null 2>&1
    fi
    if [ -f "$MUSL_STUB" ]; then
        XBIN_STUB_PATH="$MUSL_STUB"
        export XBIN_STUB_PATH
    else
        log "WARNING: no fresh stub found; xbin may embed a stale installed stub"
    fi
fi

# ---------------------------------------------------------------------------
# Machine profile
# ---------------------------------------------------------------------------
collect_profile() {
    local live="no"
    grep -qEi 'boot=live|toram' /proc/cmdline 2>/dev/null && live="yes"
    local env_type="bare-metal-or-vm"
    [ -f /.dockerenv ] && env_type="docker-container"
    [ -f /run/.containerenv ] && env_type="podman-container"
    {
        echo "# Machine profile — $MACHINE"
        echo "hostname:    $(hostname)"
        echo "kernel:      $(uname -sr)"
        echo "arch:        $(uname -m)"
        echo "cpu_model:   $(awk -F': ' '/model name/{print $2; exit}' /proc/cpuinfo)"
        echo "cores:       $(nproc)"
        echo "ram_total:   $(free -h | awk '/Mem:/{print $2}')"
        echo "root_dev:    $(findmnt -no SOURCE / 2>/dev/null || echo n/a)"
        echo "root_fs:     $(stat -f -c %T / 2>/dev/null)"
        echo "disk_total:  $(df -h / | awk 'NR==2{print $2}')"
        echo "disk_free:   $(df -h / | awk 'NR==2{print $4}')"
        echo "tmp_free:    $(df -h /tmp 2>/dev/null | awk 'NR==2{print $4}')"
        echo "env:         $env_type"
        echo "live_system: $live"
        echo "xbin:        $($XBIN_BIN --version 2>/dev/null || echo n/a)"
        echo "stub:        ${XBIN_STUB_PATH:-n/a} ($(stat -c%s "${XBIN_STUB_PATH:-/nonexistent}" 2>/dev/null || echo '?') bytes)"
        echo "node:        $(node --version 2>/dev/null || echo n/a)"
        echo "docker:      $(docker --version 2>/dev/null || echo n/a)"
        echo "pkg:         @yao-pkg/pkg@latest node24-linux-x64"
    } > "$OUT_DIR/profile.txt"
}

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
wait_http_200() {
    local port=$1 t=$2 start deadline
    start=$(date +%s)
    deadline=$((start + t))
    while :; do
        if curl -sf -o /dev/null --max-time 1 "http://127.0.0.1:$port/"; then
            return 0
        fi
        (( $(date +%s) >= deadline )) && return 1
        sleep 0.05
    done
}

now_ms() { date +%s%N | cut -c1-13; }

record() {
    local packager=$1; shift
    printf '%s\t%s\n' "$packager" "$(printf '%s;' "$@")" >> "$OUT_DIR/results.tsv"
}

MEAS_COLD_MS=""; MEAS_RSS_KB=""; MEAS_PID=""
spawn_and_measure() {
    local name=$1 port=$2 timeout_s=$3; shift 3
    local start end
    start=$(now_ms)
    "$@" >/dev/null 2>&1 &
    MEAS_PID=$!
    if wait_http_200 "$port" "$timeout_s"; then
        end=$(now_ms)
        MEAS_COLD_MS=$((end - start))
        sleep 1
        # RSS of the process actually listening on the port (the serving
        # process). Some packagers re-exec/spawn children (AppImage runtime,
        # xbin exec) — the launched PID may not be the server.
        local server_pid
        server_pid=$(ss -tlnp 2>/dev/null | grep ":$port" | grep -oP 'pid=\K[0-9]+' | head -1)
        if [ -n "$server_pid" ] && [ -r "/proc/$server_pid/status" ]; then
            MEAS_RSS_KB=$(awk '/VmRSS/{print $2}' "/proc/$server_pid/status")
        elif [ -r "/proc/$MEAS_PID/status" ]; then
            MEAS_RSS_KB=$(awk '/VmRSS/{print $2}' "/proc/$MEAS_PID/status")
        else
            MEAS_RSS_KB="n/a"
        fi
        kill "$MEAS_PID" 2>/dev/null || true
        wait "$MEAS_PID" 2>/dev/null || true
        return 0
    fi
    kill "$MEAS_PID" 2>/dev/null || true
    wait "$MEAS_PID" 2>/dev/null || true
    return 1
}

fmt_mib() {
    local b=$1
    if [ -n "$b" ] && [ "$b" != "n/a" ]; then
        awk -v b="$b" 'BEGIN{printf "%.1f", b/1048576}'
    else
        echo "n/a"
    fi
}

cell() { [ -n "$1" ] && echo "$1" || echo "n/a"; }

# ---------------------------------------------------------------------------
# x.bin
# ---------------------------------------------------------------------------
measure_xbin() {
    local port=$((PORT_BASE + 1))
    log "xbin: building"
    local t0 t1
    t0=$(now_ms)
    "$XBIN_BIN" build "$APP_DIR" -o "$WORK/hello.xbin" >/dev/null 2>&1
    t1=$(now_ms)
    local build_ms=$((t1 - t0))
    local artifact_size
    artifact_size=$(stat -c%s "$WORK/hello.xbin")

    log "xbin: cold start (first run, extraction included)"
    rm -rf "$OUT_DIR/xbin-cache"
    export XDG_CACHE_HOME="$OUT_DIR/xbin-cache"
    if spawn_and_measure xbin "$port" 60 env PORT="$port" "$WORK/hello.xbin"; then
        local cold_ms=$MEAS_COLD_MS rss_kb=$MEAS_RSS_KB
        log "xbin: warm start (cache hit)"
        if spawn_and_measure xbin "$((port + 1))" 30 env PORT="$((port + 1))" "$WORK/hello.xbin"; then
            local warm_ms=$MEAS_COLD_MS
        else
            local warm_ms="n/a"
        fi
        local footprint
        footprint=$(du -sb "$OUT_DIR/xbin-cache" 2>/dev/null | awk '{print $1}')
        unset XDG_CACHE_HOME
        record xbin "build_ms=$build_ms" "artifact_bytes=$artifact_size" \
            "artifact_mib=$(fmt_mib "$artifact_size")" \
            "footprint_bytes=$footprint" "footprint_mib=$(fmt_mib "$footprint")" \
            "cold_ms=$cold_ms" "warm_ms=$warm_ms" "rss_kb=$rss_kb" \
            "rss_mib=$(fmt_mib "$((rss_kb * 1024))")" "host_deps=none"
        return 0
    fi
    unset XDG_CACHE_HOME
    record xbin "build_ms=$build_ms" "artifact_bytes=$artifact_size" \
        "artifact_mib=$(fmt_mib "$artifact_size")" "cold_ms=fail" \
        "warm_ms=n/a" "rss_kb=n/a" "host_deps=none"
    return 1
}

# ---------------------------------------------------------------------------
# Docker
# ---------------------------------------------------------------------------
measure_docker() {
    require_cmd docker || { log "docker: skipped (not installed)"; return 1; }
    docker ps >/dev/null 2>&1 || { log "docker: skipped (daemon down)"; return 1; }
    local port=$((PORT_BASE + 2)) tag="xbin-bench-hello:$(date +%s)"
    local dockerfile="$WORK/Dockerfile"
    cat > "$dockerfile" <<EOF
FROM node:24-slim
WORKDIR /app
COPY package.json ./
COPY index.js ./
ENV PORT=$port
EXPOSE $port
CMD ["node", "index.js"]
EOF
    log "docker: building image"
    local t0 t1
    t0=$(now_ms)
    docker build -q -f "$dockerfile" -t "$tag" "$APP_DIR" >/dev/null
    t1=$(now_ms)
    local build_ms=$((t1 - t0))
    local artifact_size
    artifact_size=$(docker image inspect -f '{{.Size}}' "$tag" 2>/dev/null)
    log "docker: cold start"
    local cname="xbin-bench-$port"
    docker rm -f "$cname" >/dev/null 2>&1
    local t2 t3
    t2=$(now_ms)
    docker run -d --name "$cname" -p "$port:$port" "$tag" >/dev/null
    if wait_http_200 "$port" 90; then
        t3=$(now_ms)
        local cold_ms=$((t3 - t2))
        sleep 1
        local cpid rss_kb="n/a"
        cpid=$(docker inspect -f '{{.State.Pid}}' "$cname" 2>/dev/null)
        if [ -n "$cpid" ] && [ "$cpid" != "0" ]; then
            rss_kb=$(awk '/VmRSS/{print $2}' "/proc/$cpid/status" 2>/dev/null || echo n/a)
        fi
        docker rm -f "$cname" >/dev/null 2>&1
        record docker "build_ms=$build_ms" "artifact_bytes=$artifact_size" \
            "artifact_mib=$(fmt_mib "$artifact_size")" \
            "footprint_bytes=$artifact_size" "footprint_mib=$(fmt_mib "$artifact_size")" \
            "cold_ms=$cold_ms" "warm_ms=n/a" "rss_kb=$rss_kb" \
            "rss_mib=$(fmt_mib "$((rss_kb * 1024))")" "host_deps=docker-daemon"
        return 0
    fi
    docker rm -f "$cname" >/dev/null 2>&1
    record docker "build_ms=$build_ms" "artifact_bytes=$artifact_size" \
        "artifact_mib=$(fmt_mib "$artifact_size")" "cold_ms=fail" \
        "warm_ms=n/a" "rss_kb=n/a" "host_deps=docker-daemon"
    return 1
}

# ---------------------------------------------------------------------------
# pkg (Node.js single executable)
# ---------------------------------------------------------------------------
measure_pkg() {
    require_cmd npx || { log "pkg: skipped (npx not installed)"; return 1; }
    local port=$((PORT_BASE + 3))
    log "pkg: building via npx @yao-pkg/pkg (node24, first run downloads node base)"
    local t0 t1
    t0=$(now_ms)
    if ! npx -y @yao-pkg/pkg@latest "$APP_DIR" --targets node24-linux-x64 --output "$WORK/hello-pkg" >/dev/null 2>&1; then
        log "pkg: build failed"
        record pkg "build_ms=n/a" "artifact_bytes=n/a" "cold_ms=fail" "rss_kb=n/a" "host_deps=none"
        return 1
    fi
    t1=$(now_ms)
    local build_ms=$((t1 - t0))
    local artifact_size
    artifact_size=$(stat -c%s "$WORK/hello-pkg" 2>/dev/null)
    log "pkg: cold start"
    if spawn_and_measure pkg "$port" 30 env PORT="$port" "$WORK/hello-pkg"; then
        record pkg "build_ms=$build_ms" "artifact_bytes=$artifact_size" \
            "artifact_mib=$(fmt_mib "$artifact_size")" \
            "footprint_bytes=$artifact_size" "footprint_mib=$(fmt_mib "$artifact_size")" \
            "cold_ms=$MEAS_COLD_MS" "warm_ms=n/a" "rss_kb=$MEAS_RSS_KB" \
            "rss_mib=$(fmt_mib "$((MEAS_RSS_KB * 1024))")" "host_deps=none"
        return 0
    fi
    record pkg "build_ms=$build_ms" "artifact_bytes=$artifact_size" \
        "artifact_mib=$(fmt_mib "$artifact_size")" "cold_ms=fail" "rss_kb=n/a" "host_deps=none"
    return 1
}

# ---------------------------------------------------------------------------
# AppImage
# ---------------------------------------------------------------------------
measure_appimage() {
    require_cmd curl || { log "appimage: skipped (curl not installed)"; return 1; }
    local port=$((PORT_BASE + 4))
    local tool="$WORK/appimagetool"
    if [ ! -x "$tool" ]; then
        log "appimage: downloading appimagetool"
        curl -fsSL -o "$tool" \
            "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage" \
            || { log "appimage: download failed"; record appimage "artifact_bytes=n/a" "cold_ms=fail" "rss_kb=n/a" "host_deps=fuse"; return 1; }
        chmod +x "$tool"
    fi

    local appdir="$WORK/hello.AppDir"
    rm -rf "$appdir"
    mkdir -p "$appdir"
    if [ ! -f "$WORK/hello-pkg" ]; then
        log "appimage: skipped (pkg binary missing — run measure_pkg first)"
        record appimage "artifact_bytes=n/a" "cold_ms=fail" "rss_kb=n/a" "host_deps=fuse" "note=pkg-missing"
        return 1
    fi
    cp "$WORK/hello-pkg" "$appdir/hello-node"

    cat > "$appdir/AppRun" <<EOF
#!/bin/sh
PORT=$port
export PORT
exec "\$(dirname "\$(readlink -f "\$0")")/hello-node"
EOF
    chmod +x "$appdir/AppRun"
    cat > "$appdir/xbin-bench.desktop" <<EOF
[Desktop Entry]
Name=Hello Node
Exec=hello-node
Type=Application
Icon=xbin-bench
Categories=Utility;
EOF
    printf '%s' 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=' | base64 -d > "$appdir/xbin-bench.png"

    log "appimage: packaging"
    local t0 t1
    t0=$(now_ms)
    if ! "$tool" --appimage-extract-and-run "$appdir" "$WORK/hello-node.AppImage" >/dev/null 2>&1; then
        log "appimage: packaging failed"
        record appimage "artifact_bytes=n/a" "cold_ms=fail" "rss_kb=n/a" "host_deps=fuse"
        return 1
    fi
    t1=$(now_ms)
    local build_ms=$((t1 - t0))
    local artifact_size
    artifact_size=$(stat -c%s "$WORK/hello-node.AppImage")
    log "appimage: cold start (extract-and-run, no FUSE in container)"
    if spawn_and_measure appimage "$port" 30 "$WORK/hello-node.AppImage" --appimage-extract-and-run; then
        record appimage "build_ms=$build_ms" "artifact_bytes=$artifact_size" \
            "artifact_mib=$(fmt_mib "$artifact_size")" \
            "footprint_bytes=$artifact_size" "footprint_mib=$(fmt_mib "$artifact_size")" \
            "cold_ms=$MEAS_COLD_MS" "warm_ms=n/a" "rss_kb=$MEAS_RSS_KB" \
            "rss_mib=$(fmt_mib "$((MEAS_RSS_KB * 1024))")" "host_deps=fuse-or-extract"
        return 0
    fi
    record appimage "build_ms=$build_ms" "artifact_bytes=$artifact_size" \
        "artifact_mib=$(fmt_mib "$artifact_size")" "cold_ms=fail" "rss_kb=n/a" "host_deps=fuse"
    return 1
}

# ---------------------------------------------------------------------------
# Flatpak
# ---------------------------------------------------------------------------
measure_flatpak() {
    if ! require_cmd flatpak-builder; then
        log "flatpak: skipped (flatpak-builder not installed — needs a Flatpak host + OSTree runtimes)"
        record flatpak "artifact_bytes=n/a" "cold_ms=n/a" "rss_kb=n/a" "host_deps=flatpak" "note=not-installed"
        return 1
    fi
    record flatpak "artifact_bytes=n/a" "cold_ms=n/a" "rss_kb=n/a" "host_deps=flatpak" "note=todo"
    return 1
}

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
render_markdown() {
    local md="$OUT_DIR/comparison.md"
    {
        echo "# Comparative Benchmark — x.bin vs Docker / pkg / AppImage / Flatpak"
        echo
        echo "_Generated: $(date -u +%Y-%m-%dT%H:%MZ) — machine: \`$MACHINE\`_"
        echo "_Reference app: \`${APP_DIR}\` (Node.js HTTP server, zero deps)_"
        echo
        echo "## Test machine"
        echo
        echo '```'
        cat "$OUT_DIR/profile.txt"
        echo '```'
        echo
        echo "## Results"
        echo
        echo "| Packager | Artifact | On-disk | Cold start | Warm start | Idle RSS | Host deps |"
        echo "|----------|----------|---------|------------|------------|----------|-----------|"
        while IFS=$'\t' read -r packager rest; do
            local a b c d e f
            a=$(echo "$rest" | tr ';' '\n' | sed -n 's/.*artifact_mib=//p')
            b=$(echo "$rest" | tr ';' '\n' | sed -n 's/.*footprint_mib=//p')
            c=$(echo "$rest" | tr ';' '\n' | sed -n 's/.*cold_ms=//p')
            d=$(echo "$rest" | tr ';' '\n' | sed -n 's/.*warm_ms=//p')
            e=$(echo "$rest" | tr ';' '\n' | sed -n 's/.*rss_mib=//p')
            f=$(echo "$rest" | tr ';' '\n' | sed -n 's/.*host_deps=//p')
            echo "| $packager | $(cell "$a") MiB | $(cell "$b") MiB | $(cell "$c") ms | $(cell "$d") ms | $(cell "$e") MiB | $(cell "$f") |"
        done < "$OUT_DIR/results.tsv"
        echo
        echo "## Methodology"
        echo
        echo "- **Cold start** = wall time from launch to first HTTP 200."
        echo "- **Warm start** = second launch of the same artifact (extraction cache hit). Only x.bin caches; the other packagers re-launch every time."
        echo "- **Idle RSS** = resident set of the process actually listening on the port, 1s after first response (Linux VmRSS, resolved via \`ss\`). Some packagers re-exec/spawn children (AppImage runtime, x.bin exec) — the server process is measured, not the launched PID."
        echo "- **On-disk footprint** = space used at run time (x.bin: extracted rootfs cache; Docker: uncompressed image; pkg/AppImage: the artifact itself)."
        echo "- **Host deps** = packages/services the target host must provide."
        echo "- Flatpak requires a Flatpak host + OSTree runtimes; not measured in this container."
        echo "- Every run records a **machine profile** (\`results/<machine>/profile.txt\`) — comparing two machines without their profile is meaningless."
        echo
        echo "Raw data: \`results.tsv\`. Re-run: \`bash benchmarks/comparison/run.sh\`."
        echo "Multi-machine aggregation: \`bash benchmarks/comparison/aggregate.sh\`."
    } > "$md"
    echo "$md"
}

main() {
    collect_profile
    log "machine: $MACHINE  app: $APP_DIR  out: $OUT_DIR"
    cat "$OUT_DIR/profile.txt"
    measure_xbin || true
    measure_docker || true
    measure_pkg || true
    measure_appimage || true
    measure_flatpak || true
    local md
    md=$(render_markdown)
    log "done. results: $md"
}

main
