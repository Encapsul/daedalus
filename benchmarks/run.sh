#!/usr/bin/env bash
# benchmarks/run.sh — x.bin vs native benchmark suite
#
# Reproducible benchmarks for xbin against 3 real-world Python apps.
# Produces: benchmarks/report.md + benchmarks/results.json
#
# Usage: bash benchmarks/run.sh
#
# Prerequisites: python3 >= 3.10, rustc (musl target), zstd, curl
# Target repos should be in repo/ (yt-dlp, whisper, glances/open-webui proxy).
#
# RSS is measured via /proc/$pid/status VmRSS — this is the kernel's actual
# RSS value, equivalent to what /usr/bin/time -v reports as "Maximum resident
# set size". We use it directly because /usr/bin/time -v can't measure a
# long-running server's idle RSS (it waits for the command to finish).
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
REPOS_DIR="$REPO_ROOT/repo"
RESULTS_DIR="$SCRIPT_DIR/results"
REPORT_MD="$SCRIPT_DIR/report.md"
RESULTS_JSON="$SCRIPT_DIR/results.json"

# Use /tmp for all build artifacts (FAT32 can't do symlinks/chmod).
WORK="/tmp/xbin-bench"
APPS_DIR="$WORK/apps"
PORT_BASE=19800

# Xbin command — not installed globally, run via module.
xbin() {
    PYTHONPATH="$REPO_ROOT/cli" python3 -m xbin "$@"
}

# Detect cargo bin (for stub build).
CARGO_BIN="$(dirname "$(which rustc 2>/dev/null || echo /usr/bin/rustc)")"

# Timeouts (seconds) — generous for heavy ML installs.
TIMEOUT_VENV_INSTALL=600
TIMEOUT_COLD_START=120
TIMEOUT_WARM_START=60

# Colors
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[0;33m'
CYAN='\033[0;36m'; BOLD='\033[1m'; NC='\033[0m'

log()  { echo -e "${CYAN}[bench]${NC} $*"; }
warn() { echo -e "${YELLOW}[warn]${NC} $*"; }
fail() { echo -e "${RED}[FAIL]${NC} $*"; }
ok()   { echo -e "${GREEN}[OK]${NC} $*"; }
header() { echo -e "\n${BOLD}=== $* ===${NC}"; }

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

get_rss_kb() {
    # Read VmRSS from /proc/$pid/status (kernel's actual RSS, not VSZ).
    awk '/VmRSS/ {print $2}' /proc/$1/status 2>/dev/null || echo "0"
}

kill_port() { fuser -k "$1/tcp" 2>/dev/null || true; }

kill_all_ports() {
    for p in $(seq "$PORT_BASE" $((PORT_BASE + 20))); do
        kill_port "$p" 2>/dev/null || true
    done
}

wait_for_ready() {
    local port=$1 timeout=${2:-60} bpid=${3:-}
    local start end now
    start=$(date +%s)
    while true; do
        if [ -n "$bpid" ] && ! kill -0 "$bpid" 2>/dev/null; then
            return 1
        fi
        if curl -sf "http://127.0.0.1:$port/" > /dev/null 2>&1; then
            return 0
        fi
        now=$(date +%s)
        if (( now - start > timeout )); then
            return 1
        fi
        sleep 0.5
    done
}

cleanup() {
    log "Cleaning up background processes..."
    kill_all_ports
    # Kill any leftover python/http servers we started
    for pidfile in "$WORK"/*.pid; do
        [ -f "$pidfile" ] || continue
        local pid
        pid=$(cat "$pidfile" 2>/dev/null)
        if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
            kill "$pid" 2>/dev/null || true
        fi
        rm -f "$pidfile"
    done
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# App definitions: each setup_* creates app.py + requirements in $APPS_DIR/<name>
# ---------------------------------------------------------------------------

setup_ytdlp() {
    local d="$APPS_DIR/yt-dlp"
    rm -rf "$d"; mkdir -p "$d"

    # Real yt-dlp library, wrapped as HTTP server for benchmarking.
    cat > "$d/app.py" <<'PYEOF'
import http.server, json, os, sys
import yt_dlp
PORT = int(os.environ.get("PORT", "19800"))
class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/":
            body = json.dumps({
                "status": "ok",
                "version": yt_dlp.version.__version__,
                "python": sys.version.split()[0],
                "pid": os.getpid()
            }).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        else:
            self.send_response(404)
            self.end_headers()
    def log_message(self, *_): pass
if __name__ == "__main__":
    s = http.server.HTTPServer(("127.0.0.1", PORT), Handler)
    print(f"READY :{PORT}", flush=True)
    s.serve_forever()
PYEOF
    echo "yt-dlp" > "$d/requirements.txt"
    echo "$d"
}

setup_openwebui() {
    # Open WebUI is a large Next.js + Python app that can't be pip-installed
    # standalone. We use a PROXY that imports the same heavy deps (torch,
    # transformers, numpy) to measure xbin's overhead on a realistic ML stack.
    local d="$APPS_DIR/open-webui"
    rm -rf "$d"; mkdir -p "$d"

    cat > "$d/app.py" <<'PYEOF'
import http.server, json, os, sys
import torch, numpy, transformers
PORT = int(os.environ.get("PORT", "19801"))
class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/":
            body = json.dumps({
                "status": "ok",
                "torch": torch.__version__,
                "numpy": numpy.__version__,
                "transformers": transformers.__version__,
                "cuda": torch.cuda.is_available(),
                "python": sys.version.split()[0],
                "pid": os.getpid()
            }).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        else:
            self.send_response(404)
            self.end_headers()
    def log_message(self, *_): pass
if __name__ == "__main__":
    s = http.server.HTTPServer(("127.0.0.1", PORT), Handler)
    print(f"READY :{PORT}", flush=True)
    s.serve_forever()
PYEOF
    cat > "$d/requirements.txt" <<'REQEOF'
--extra-index-url https://download.pytorch.org/whl/cpu
torch
numpy
transformers
REQEOF
    echo "$d"
}

setup_whisper() {
    local d="$APPS_DIR/whisper"
    rm -rf "$d"; mkdir -p "$d"

    cat > "$d/app.py" <<'PYEOF'
import http.server, json, os, sys
import whisper, numpy as np
PORT = int(os.environ.get("PORT", "19802"))
class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/":
            body = json.dumps({
                "status": "ok",
                "whisper": whisper.__version__,
                "numpy": np.__version__,
                "models": whisper.available_models(),
                "python": sys.version.split()[0],
                "pid": os.getpid()
            }).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        else:
            self.send_response(404)
            self.end_headers()
    def log_message(self, *_): pass
if __name__ == "__main__":
    s = http.server.HTTPServer(("127.0.0.1", PORT), Handler)
    print(f"READY :{PORT}", flush=True)
    s.serve_forever()
PYEOF
    cat > "$d/requirements.txt" <<'REQEOF'
--extra-index-url https://download.pytorch.org/whl/cpu
openai-whisper
REQEOF
    echo "$d"
}

# ---------------------------------------------------------------------------
# Baseline: native venv + pip install + cold start + RSS
# ---------------------------------------------------------------------------

run_baseline() {
    local name="$1" app_dir="$2" port="$3" install_extra="${4:-}"
    header "BASELINE: $name"

    local venv="$WORK/venvs/$name"
    rm -rf "$venv"

    # 1) Fresh venv + pip install
    log "Creating venv and installing dependencies..."
    local t0 t1 install_time
    t0=$(python3 -c "import time; print(time.monotonic())")

    if ! timeout "$TIMEOUT_VENV_INSTALL" python3 -m venv --clear "$venv" 2>&1; then
        fail "venv creation failed"
        echo "{\"app\":\"$name\",\"phase\":\"baseline\",\"error\":\"venv creation failed\"}" \
            > "$RESULTS_DIR/${name}-baseline.json"
        return 1
    fi

    # Install from repo if it exists, otherwise use install_extra
    if [ -d "$REPOS_DIR/$name" ] && [ -f "$REPOS_DIR/$name/pyproject.toml" ]; then
        log "Installing from $REPOS_DIR/$name..."
        if ! timeout "$TIMEOUT_VENV_INSTALL" "$venv/bin/pip" install \
            --quiet --no-cache-dir "$REPOS_DIR/$name" 2>&1 | tail -20; then
            fail "pip install from repo failed"
            echo "{\"app\":\"$name\",\"phase\":\"baseline\",\"error\":\"pip install failed\"}" \
                > "$RESULTS_DIR/${name}-baseline.json"
            return 1
        fi
    elif [ -n "$install_extra" ]; then
        log "Installing: $install_extra..."
        if ! timeout "$TIMEOUT_VENV_INSTALL" "$venv/bin/pip" install \
            --quiet --no-cache-dir $install_extra 2>&1 | tail -20; then
            fail "pip install failed"
            echo "{\"app\":\"$name\",\"phase\":\"baseline\",\"error\":\"pip install failed: $install_extra\"}" \
                > "$RESULTS_DIR/${name}-baseline.json"
            return 1
        fi
    else
        fail "no install method for $name"
        return 1
    fi

    t1=$(python3 -c "import time; print(time.monotonic())")
    install_time=$(printf "%.2f" "$(echo "$t1 - $t0" | bc)")
    ok "install: ${install_time}s"

    # 2) Venv disk size
    local venv_bytes venv_human
    venv_bytes=$(du -sb "$venv" | awk '{print $1}')
    venv_human=$(numfmt --to=iec "$venv_bytes" 2>/dev/null || echo "${venv_bytes}B")
    ok "venv size: $venv_human"

    # 3) Cold start — measure time to first HTTP response
    kill_port "$port"; sleep 0.5
    log "Measuring cold start (port $port)..."
    local t0 t1 cold
    t0=$(python3 -c "import time; print(time.monotonic())")

    "$venv/bin/python" "$app_dir/app.py" &
    local bpid=$!
    echo "$bpid" > "$WORK/${name}-baseline.pid"

    if ! wait_for_ready "$port" "$TIMEOUT_COLD_START" "$bpid"; then
        fail "baseline $name: server did not start within ${TIMEOUT_COLD_START}s"
        # Capture any error output
        if ! kill -0 "$bpid" 2>/dev/null; then
            wait "$bpid" 2>/dev/null || true
        fi
        kill "$bpid" 2>/dev/null; wait "$bpid" 2>/dev/null || true
        rm -f "$WORK/${name}-baseline.pid"
        echo "{\"app\":\"$name\",\"phase\":\"baseline\",\"error\":\"server did not start\",\"install_time_s\":$install_time,\"venv_size_bytes\":$venv_bytes}" \
            > "$RESULTS_DIR/${name}-baseline.json"
        return 1
    fi

    t1=$(python3 -c "import time; print(time.monotonic())")
    cold=$(printf "%.2f" "$(echo "$t1 - $t0" | bc)")
    ok "cold start: ${cold}s"

    # 4) RSS at idle — read from /proc (kernel's actual RSS, not VSZ)
    sleep 2  # let server settle
    local rss
    rss=$(get_rss_kb "$bpid")
    ok "RSS at idle: ${rss} KB"

    # 5) PSS (proportional set size) if available — more accurate for shared libs
    local pss=0
    if [ -f "/proc/$bpid/smaps_rollup" ]; then
        pss=$(awk '/^Pss:/ {print $2; exit}' "/proc/$bpid/smaps_rollup" 2>/dev/null || echo "0")
    fi

    # Cleanup
    kill "$bpid" 2>/dev/null; wait "$bpid" 2>/dev/null || true
    rm -f "$WORK/${name}-baseline.pid"
    kill_port "$port"

    # Write JSON
    cat > "$RESULTS_DIR/${name}-baseline.json" <<JEOF
{
  "app": "$name", "phase": "baseline",
  "install_time_s": $install_time,
  "venv_size_bytes": $venv_bytes, "venv_size_human": "$venv_human",
  "cold_start_time_s": $cold,
  "rss_kb": $rss, "pss_kb": $pss
}
JEOF
    ok "baseline done: $name"
}

# ---------------------------------------------------------------------------
# xbin: build + cold start + warm start + RSS + cache size
# ---------------------------------------------------------------------------

run_xbin() {
    local name="$1" app_dir="$2" port="$3"
    header "XBIN: $name"

    local tmp_xbin="$WORK/${name}.xbin"

    # 1) Build .xbin
    log "Building .xbin for $name..."
    local t0 t1 build_time
    t0=$(python3 -c "import time; print(time.monotonic())")

    cd "$REPO_ROOT"
    if ! xbin build "$app_dir" -o "$tmp_xbin" 2>&1 | tail -20; then
        fail "xbin build failed for $name"
        echo "{\"app\":\"$name\",\"phase\":\"xbin\",\"error\":\"build failed\"}" \
            > "$RESULTS_DIR/${name}-xbin.json"
        return 1
    fi

    t1=$(python3 -c "import time; print(time.monotonic())")
    build_time=$(printf "%.2f" "$(echo "$t1 - $t0" | bc)")
    ok "build: ${build_time}s"

    # 2) .xbin size
    local xb_bytes xb_human
    xb_bytes=$(stat -c%s "$tmp_xbin")
    xb_human=$(numfmt --to=iec "$xb_bytes" 2>/dev/null || echo "${xb_bytes}B")
    ok ".xbin size: $xb_human"

    # 3) Cold start — clear cache, measure extraction + launch
    log "Cold start (clearing cache)..."
    xbin clean --all 2>/dev/null || true
    kill_port "$port"; sleep 1

    local t0 t1 cold
    t0=$(python3 -c "import time; print(time.monotonic())")

    chmod +x "$tmp_xbin"
    "$tmp_xbin" &
    local bpid=$!
    echo "$bpid" > "$WORK/${name}-xbin.pid"

    if ! wait_for_ready "$port" "$TIMEOUT_COLD_START" "$bpid"; then
        fail "xbin $name: cold start failed (${TIMEOUT_COLD_START}s)"
        kill "$bpid" 2>/dev/null; wait "$bpid" 2>/dev/null || true
        rm -f "$WORK/${name}-xbin.pid"
        echo "{\"app\":\"$name\",\"phase\":\"xbin\",\"error\":\"cold start timed out\",\"build_time_s\":$build_time,\"xbin_size_bytes\":$xb_bytes}" \
            > "$RESULTS_DIR/${name}-xbin.json"
        return 1
    fi

    t1=$(python3 -c "import time; print(time.monotonic())")
    cold=$(printf "%.2f" "$(echo "$t1 - $t0" | bc)")
    ok "cold start: ${cold}s"

    # 4) RSS at idle (cold)
    sleep 2
    local rss_cold
    rss_cold=$(get_rss_kb "$bpid")
    ok "RSS (cold): ${rss_cold} KB"

    # 5) Warm start — kill, re-launch (cache hit)
    log "Warm start (cache hit)..."
    kill "$bpid" 2>/dev/null; wait "$bpid" 2>/dev/null || true
    rm -f "$WORK/${name}-xbin.pid"
    kill_port "$port"; sleep 1

    t0=$(python3 -c "import time; print(time.monotonic())")

    "$tmp_xbin" &
    bpid=$!
    echo "$bpid" > "$WORK/${name}-xbin.pid"

    if ! wait_for_ready "$port" "$TIMEOUT_WARM_START" "$bpid"; then
        fail "xbin $name: warm start timed out"
        kill "$bpid" 2>/dev/null; wait "$bpid" 2>/dev/null || true
        rm -f "$WORK/${name}-xbin.pid"
        echo "{\"app\":\"$name\",\"phase\":\"xbin\",\"error\":\"warm start timed out\",\"build_time_s\":$build_time,\"xbin_size_bytes\":$xb_bytes,\"cold_start_time_s\":$cold}" \
            > "$RESULTS_DIR/${name}-xbin.json"
        return 1
    fi

    t1=$(python3 -c "import time; print(time.monotonic())")
    local warm
    warm=$(printf "%.2f" "$(echo "$t1 - $t0" | bc)")
    ok "warm start: ${warm}s"

    # 6) RSS at idle (warm)
    sleep 2
    local rss_warm
    rss_warm=$(get_rss_kb "$bpid")
    ok "RSS (warm): ${rss_warm} KB"

    # Cleanup
    kill "$bpid" 2>/dev/null; wait "$bpid" 2>/dev/null || true
    rm -f "$WORK/${name}-xbin.pid"
    kill_port "$port"

    # 7) Cache directory size
    local cache_size=0 cache_human="0B"
    if [ -d "$HOME/.cache/xbin" ]; then
        cache_size=$(du -sb "$HOME/.cache/xbin" | awk '{print $1}')
        cache_human=$(numfmt --to=iec "$cache_size" 2>/dev/null || echo "${cache_size}B")
    fi
    ok "cache size: $cache_human"

    # Use the larger of cold/warm RSS (should be near-identical)
    local rss_final=$rss_cold
    [ "${rss_warm:-0}" -gt "${rss_final:-0}" ] 2>/dev/null && rss_final=$rss_warm

    cat > "$RESULTS_DIR/${name}-xbin.json" <<JEOF
{
  "app": "$name", "phase": "xbin",
  "build_time_s": $build_time,
  "xbin_size_bytes": $xb_bytes, "xbin_size_human": "$xb_human",
  "cold_start_time_s": $cold, "warm_start_time_s": $warm,
  "rss_cold_kb": $rss_cold, "rss_warm_kb": $rss_warm,
  "rss_kb": $rss_final,
  "cache_size_bytes": $cache_size, "cache_size_human": "$cache_human"
}
JEOF
    ok "xbin done: $name"
}

# ---------------------------------------------------------------------------
# Report generator
# ---------------------------------------------------------------------------

generate_report() {
    log "Generating report..."
    python3 - "$RESULTS_DIR" "$REPORT_MD" "$RESULTS_JSON" "$REPO_ROOT" <<'PYEOF'
import json, os, sys
from datetime import datetime, timezone

RESULTS_DIR, REPORT_MD, RESULTS_JSON, REPO_ROOT = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
apps = ["yt-dlp", "open-webui", "whisper"]
all_r = {}

for a in apps:
    all_r[a] = {}
    for p in ["baseline", "xbin"]:
        path = f"{RESULTS_DIR}/{a}-{p}.json"
        if os.path.exists(path):
            with open(path) as f:
                try:
                    all_r[a][p] = json.load(f)
                except json.JSONDecodeError:
                    all_r[a][p] = {"error": "invalid JSON"}

with open(RESULTS_JSON, "w") as f:
    json.dump(all_r, f, indent=2)

def fmt_s(t):
    if t is None: return "N/A"
    return f"{t:.2f}s"

def fmt_b(b):
    if b is None or b == 0: return "N/A"
    for u, d in [(1e9,"GB"),(1e6,"MB"),(1e3,"KB")]:
        if b >= u: return f"{b/u:.1f} {d}"
    return f"{b} B"

def fmt_kb(k):
    if k is None or k == 0: return "N/A"
    if k >= 1024: return f"{k/1024:.1f} MB"
    return f"{k} KB"

def pct(old, new):
    if old is None or new is None or old == 0: return "—"
    r = new / old
    return f"+{(r-1)*100:.0f}%" if r > 1 else f"-{(1-r)*100:.0f}%"

# Gather system info
import subprocess
def run(cmd):
    try:
        return subprocess.run(cmd, shell=True, capture_output=True, text=True, timeout=5).stdout.strip()
    except:
        return "N/A"

now = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC")
hostname = run("hostname")
nproc = run("nproc")
ram = run("free -h | awk '/Mem:/ {print $2}'")
py_ver = run("python3 --version 2>&1")
rust_ver = run("rustc --version 2>&1")
zstd_ver = run("zstd --version 2>&1 | head -1")

lines = []
lines.append("# x.bin Benchmark Report\n")
lines.append(f"**Date:** {now}  ")
lines.append(f"**Machine:** `{hostname}`  ")
lines.append(f"**CPU:** {nproc} cores  ")
lines.append(f"**RAM:** {ram}  ")
disk_free = run("df -h /tmp | tail -1 | awk '{print $4}'")
lines.append(f"**Disk free:** {disk_free}\n")

lines.append("```")
lines.append(f"Python:  {py_ver}")
lines.append(f"Rust:    {rust_ver}")
lines.append(f"zstd:    {zstd_ver}")
lines.append("```\n")

lines.append("**Apps:**\n")
lines.append("- **yt-dlp** — light case, pure-Python CLI library, few deps")
lines.append("- **open-webui** — proxy app (torch + transformers + numpy), heavy ML stack")
lines.append("- **whisper** — hard case, torch + numpy + tiktoken, heaviest deps\n")

lines.append("**Methodology:**\n")
lines.append("- RSS measured via `/proc/$pid/status` VmRSS (kernel's actual RSS, not VSZ)")
lines.append("- Cold start = process launch to first HTTP 200 response")
lines.append("- Warm start = subsequent launch with cache hit")
lines.append("- Baseline = fresh venv + pip install (no xbin)")
lines.append("- All times are single runs (not averaged) — reproducible via this script\n")

for a in apps:
    bl = all_r[a].get("baseline")
    xb = all_r[a].get("xbin")
    lines.append(f"\n---\n\n## {a}\n")

    if bl is None and xb is None:
        lines.append("**Result:** SKIPPED (no data)\n")
        continue

    if bl and bl.get("error"):
        lines.append(f"**Baseline error:** {bl['error']}\n")
    if xb and xb.get("error"):
        lines.append(f"**xbin error:** {xb['error']}\n")

    lines.append("| Metric | Baseline (native) | xbin | Delta |")
    lines.append("|--------|------------------:|-----:|------:|")

    lines.append(f"| Install / build time | "
        f"{fmt_s(bl.get('install_time_s') if bl else None)} | "
        f"{fmt_s(xb.get('build_time_s') if xb else None)} | "
        f"{pct(bl.get('install_time_s') if bl else None, xb.get('build_time_s') if xb else None)} |")

    lines.append(f"| Artifact size | "
        f"{fmt_b(bl.get('venv_size_bytes') if bl else None)} | "
        f"{fmt_b(xb.get('xbin_size_bytes') if xb else None)} | "
        f"{pct(bl.get('venv_size_bytes') if bl else None, xb.get('xbin_size_bytes') if xb else None)} |")

    lines.append(f"| Cold start | "
        f"{fmt_s(bl.get('cold_start_time_s') if bl else None)} | "
        f"{fmt_s(xb.get('cold_start_time_s') if xb else None)} | "
        f"{pct(bl.get('cold_start_time_s') if bl else None, xb.get('cold_start_time_s') if xb else None)} |")

    lines.append(f"| Warm start | "
        f"— | "
        f"{fmt_s(xb.get('warm_start_time_s') if xb else None)} | "
        f"— |")

    bl_rss = bl.get('rss_kb') if bl else None
    xb_rss = xb.get('rss_kb') if xb else None
    rss_pct = pct(bl_rss, xb_rss) if bl_rss and xb_rss else "—"
    lines.append(f"| RSS at idle | "
        f"{fmt_kb(bl_rss)} | "
        f"{fmt_kb(xb_rss)} | "
        f"{rss_pct} |")

    if xb and xb.get("cache_size_bytes"):
        lines.append(f"| Cache size (extracted) | — | {fmt_b(xb.get('cache_size_bytes'))} | — |")

    # Honest analysis
    if bl and xb and not bl.get("error") and not xb.get("error"):
        bl_cold = bl.get('cold_start_time_s', 0)
        xb_cold = xb.get('cold_start_time_s', 0)
        xb_warm = xb.get('warm_start_time_s', 0)
        if xb_warm and bl_cold and xb_warm > 0:
            overhead = xb_warm / bl_cold if bl_cold else 0
            lines.append(f"\n*xbin warm start overhead vs native cold: {overhead:.1f}x*")

    lines.append("")

lines.append("\n---\n\n## Notes\n")
lines.append("- **open-webui** is a proxy: we install torch+transformers+numpy directly")
lines.append("  (the real Open WebUI is a large Next.js app that can't be pip-installed standalone).")
lines.append("  This measures xbin's overhead on the same heavy ML dependency chain.")
lines.append("- **Cold start** includes extraction (zstd decompression + disk write) on first run.")
lines.append("- **Warm start** is cache hit — no extraction, just launcher overhead + app boot.")
lines.append("- **RSS** should be near-identical between baseline and xbin (same Python runtime).")
lines.append("  If xbin RSS is significantly higher, that's a real finding worth investigating.")
lines.append("- **Failures are reported honestly.** A hidden dependency (subprocess, dlopen)")
lines.append("  that breaks the build is more useful than a clean number that hides a gap.")
lines.append("")

with open(REPORT_MD, "w") as f:
    f.write("\n".join(lines))
print(f"Report: {REPORT_MD}")
print(f"JSON:   {RESULTS_JSON}")
PYEOF
}

# ---------------------------------------------------------------------------
# MAIN
# ---------------------------------------------------------------------------

main() {
    echo -e "${BOLD}"
    echo "============================================"
    echo " x.bin vs native benchmark suite"
    echo "============================================"
    echo -e "${NC}"

    # Verify prerequisites
    for cmd in python3 rustc zstd curl bc; do
        if ! command -v "$cmd" &>/dev/null; then
            fail "Required command not found: $cmd"
            exit 1
        fi
    done

    # Clean previous results and work dir
    rm -rf "$WORK"
    mkdir -p "$WORK/venvs" "$APPS_DIR" "$RESULTS_DIR"
    cleanup 2>/dev/null || true

    # Ensure stub is built
    local stub_path="/tmp/xbin-stub-target/x86_64-unknown-linux-musl/release/xbin-stub"
    if [ ! -f "$stub_path" ]; then
        log "Building xbin stub..."
        cd "$REPO_ROOT/stub" && cargo build --release --target x86_64-unknown-linux-musl 2>&1 | tail -5
    fi
    ok "xbin stub ready at $stub_path"

    # ---- App 1: yt-dlp (light) ----
    local ytdlp_dir
    ytdlp_dir=$(setup_ytdlp)
    run_baseline "yt-dlp" "$ytdlp_dir" "$PORT_BASE" || true
    run_xbin    "yt-dlp" "$ytdlp_dir" "$PORT_BASE" || true

    # ---- App 2: open-webui proxy (heavy ML) ----
    local owu_dir
    owu_dir=$(setup_openwebui)
    run_baseline "open-webui" "$owu_dir" "$((PORT_BASE+1))" \
        "--extra-index-url https://download.pytorch.org/whl/cpu torch numpy transformers" || true
    run_xbin "open-webui" "$owu_dir" "$((PORT_BASE+1))" || true

    # ---- App 3: whisper (hard, heavy ML) ----
    local wp_dir
    wp_dir=$(setup_whisper)
    run_baseline "whisper" "$wp_dir" "$((PORT_BASE+2))" \
        "--extra-index-url https://download.pytorch.org/whl/cpu $REPOS_DIR/whisper" || true
    run_xbin "whisper" "$wp_dir" "$((PORT_BASE+2))" || true

    # ---- Generate report ----
    generate_report

    echo ""
    echo -e "${BOLD}============================================${NC}"
    echo -e "${BOLD} Benchmark complete!${NC}"
    echo -e "${BOLD}============================================${NC}"
    echo " Report: $REPORT_MD"
    echo " JSON:   $RESULTS_JSON"
    echo ""
}

main "$@"
