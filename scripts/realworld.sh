#!/usr/bin/env bash
# realworld.sh — clone real-world apps per language and test daedalus build
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
DAEDALUS="${DAEDALUS:-$REPO_ROOT/target/release/daedalus}"
WORKDIR="${WORKDIR:-/tmp/daedalus-realworld}"
MAX_APPS_PER_LANG="${MAX_APPS_PER_LANG:-10}"

mkdir -p "$WORKDIR"
cd "$WORKDIR"

pass=0
fail=0
skipped=0

RESULTS=()

run_build() {
  local lang="$1"
  local repo="$2"
  local dir="$3"
  local extra="${4:-}"

  if [[ ! -d "$dir/.git" ]]; then
    echo "  cloning..."
    if ! git clone --depth=1 "$repo" "$dir" >/dev/null 2>&1; then
      echo "  FAIL (clone failed)"
      fail=$((fail + 1))
      RESULTS+=("FAIL $lang $repo (clone failed)")
      return 0
    fi
  fi

  echo "== $lang: $repo =="

  if [[ -n "$extra" ]]; then
    # Check if extra command is available
    if ! command -v "${extra%% *}" &>/dev/null; then
      echo "  SKIP (missing: $extra)"
      skipped=$((skipped + 1))
      return 0
    fi
  fi

  local args=("build" "$dir" "--dry-run" "--plain")
  if [[ -n "$extra" ]]; then
    args+=("--embed-interpreter" "$extra")
  fi

  set +e
  "$DAEDALUS" "${args[@]}" 2>&1 | tail -5
  local rc=$?
  set -e

  if [[ $rc -eq 0 ]]; then
    echo "  PASS"
    pass=$((pass + 1))
    RESULTS+=("PASS $lang $repo")
  else
    echo "  FAIL (exit $rc)"
    fail=$((fail + 1))
    RESULTS+=("FAIL $lang $repo (exit $rc)")
  fi
}

# ── Python ────────────────────────────────────────────────────────────────
PYTHON_REPOS=(
  "https://github.com/Textualize/rich"
  "https://github.com/tiangolo/fastapi"
  "https://github.com/django/django"
  "https://github.com/pallets/flask"
  "https://github.com/streamlit/streamlit"
  "https://github.com/sqlmapproject/sqlmap"
  "https://github.com/pybids/pybids"
  "https://github.com/encode/httpx"
  "https://github.com/tiangolo/uvicorn"
  "https://github.com/ultralytics/yolov5"
)
for repo in "${PYTHON_REPOS[@]}"; do
  name=$(basename "$repo")
  run_build "python" "$repo" "$WORKDIR/python/$name" "python3"
done

# ── Node.js ───────────────────────────────────────────────────────────────
NODE_REPOS=(
  "https://github.com/facebook/react"
  "https://github.com/expressjs/express"
  "https://github.com/nestjs/nest"
  "https://github.com/fastify/fastify"
  "https://github.com/honojs/hono"
  "https://github.com/vercel/next.js"
  "https://github.com/remix-run/remix"
  "https://github.com/sveltejs/svelte"
  "https://github.com/angular/angular"
  "https://github.com/vuejs/core"
)
for repo in "${NODE_REPOS[@]}"; do
  name=$(basename "$repo")
  run_build "node" "$repo" "$WORKDIR/node/$name" "node"
done

# ── Rust ──────────────────────────────────────────────────────────────────
RUST_REPOS=(
  "https://github.com/rust-lang/rust"
  "https://github.com/WebAssembly/WASI"
  "https://github.com/smol-rs/smol"
  "https://github.com/tokio-rs/tokio"
  "https://github.com/actix/actix-web"
  "https://github.com/bevyengine/bevy"
  "https://github.com/rust-lang/mdBook"
  "https://github.com/alacritty/alacritty"
  "https://github.com/helix-editor/helix"
  "https://github.com/zed-industries/zed"
)
for repo in "${RUST_REPOS[@]}"; do
  name=$(basename "$repo")
  run_build "rust" "$repo" "$WORKDIR/rust/$name" "cargo"
done

# ── Go ────────────────────────────────────────────────────────────────────
GO_REPOS=(
  "https://github.com/golang/go"
  "https://github.com/gin-gonic/gin"
  "https://github.com/gofiber/fiber"
  "https://github.com/ehang-io/nps"
  "https://github.com/istio/istio"
  "https://github.com/kubernetes/kubernetes"
  "https://github.com/docker/compose"
  "https://github.com/prometheus/prometheus"
  "https://github.com/grafana/k6"
  "https://github.com/cosmos/cosmos-sdk"
)
for repo in "${GO_REPOS[@]}"; do
  name=$(basename "$repo")
  run_build "go" "$repo" "$WORKDIR/go/$name" "go"
done

# ── Java ──────────────────────────────────────────────────────────────────
JAVA_REPOS=(
  "https://github.com/spring-projects/spring-boot"
  "https://github.com/quarkusio/quarkus"
  "https://github.com/micronaut-projects/micronaut-core"
  "https://github.com/apache/kafka"
  "https://github.com/elastic/elasticsearch"
  "https://github.com/airbytehq/airbyte"
  "https://github.com/GoogleContainerTools/jib"
  "https://github.com/bitcoinj/bitcoinj"
  "https://github.com/NationalSecurityAgency/ghidra"
  "https://github.com/mockito/mockito"
)
for repo in "${JAVA_REPOS[@]}"; do
  name=$(basename "$repo")
  run_build "java" "$repo" "$WORKDIR/java/$name" "mvn"
done

# ── Ruby ──────────────────────────────────────────────────────────────────
RUBY_REPOS=(
  "https://github.com/rails/rails"
  "https://github.com/sinatra/sinatra"
  "https://github.com/jekyll/jekyll"
  "https://github.com/huginn/huginn"
  "https://github.com/forem/forem"
  "https://github.com/discourse/discourse"
  "https://github.com/mastodon/mastodon"
  "https://github.com/lobsters/lobsters"
  "https://github.com/puppetlabs/puppet"
  "https://github.com/chef/chef"
)
for repo in "${RUBY_REPOS[@]}"; do
  name=$(basename "$repo")
  run_build "ruby" "$repo" "$WORKDIR/ruby/$name" "bundle"
done

# ── PHP ───────────────────────────────────────────────────────────────────
PHP_REPOS=(
  "https://github.com/laravel/laravel"
  "https://github.com/symfony/symfony"
  "https://github.com/wordpress/wordpress"
  "https://github.com/composer/composer"
  "https://github.com/php/doc"
  "https://github.com/egulias/email-validator"
  "https://github.com/nikic/PHP-Parser"
  "https://github.com/phpstan/phpstan"
  "https://github.com/php-fig/container"
  "https://github.com/spatie/laravel"
)
for repo in "${PHP_REPOS[@]}"; do
  name=$(basename "$repo")
  run_build "php" "$repo" "$WORKDIR/php/$name" "composer"
done

# ── Deno ──────────────────────────────────────────────────────────────────
DENO_REPOS=(
  "https://github.com/denoland/deno"
  "https://github.com/denoland/fresh"
  "https://github.com/litegraph/litegraph.js"
  "https://github.com/automerge/automerge"
  "https://github.com/tsirysndr/dircmds"
  "https://github.com/denoland/deployctl"
  "https://github.com/denoland/deno_std"
  "https://github.com/denoland/deno_lint"
  "https://github.com/ker0x/cobalt-cli"
  "https://github.com/loicschr/deno-bump"
)
for repo in "${DENO_REPOS[@]}"; do
  name=$(basename "$repo")
  run_build "deno" "$repo" "$WORKDIR/deno/$name" "deno"
done

# ── Hugo ──────────────────────────────────────────────────────────────────
HUGO_REPOS=(
  "https://github.com/gohugoio/hugo"
  "https://github.com/theNewDynamic/gohugo-theme-ananke"
  "https://github.com/chipzoller/hugo-clarity"
  "https://github.com/adityatelange/hugo-PaperMod"
  "https://github.com/gcusin/hugo-init"
  "https://github.com/rhazdon/hugo-theme-hello-friend"
  "https://github.com/luizdepra/hugo-coder"
  "https://github.com/panr/hugo-theme-hello-friend"
  "https://github.com/olOwOlo/hugo-theme-even"
  "https://github.com/halogenica/beautifulhugo"
)
for repo in "${HUGO_REPOS[@]}"; do
  name=$(basename "$repo")
  run_build "hugo" "$repo" "$WORKDIR/hugo/$name" "hugo"
done

echo ""
echo "═══════════════════════════════════════════"
echo "Results: $pass pass, $fail fail, $skipped skipped"
echo "═══════════════════════════════════════════"
for r in "${RESULTS[@]}"; do
  echo "$r"
done
