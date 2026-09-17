#!/bin/sh
# Rebuild all buildable hub apps from catalog.json into hub/dist/.
#
#   hub/build.sh                  build all buildable apps
#   hub/build.sh hello-web        build one app by id
#
# Requires the daedalus CLI on PATH (or a stub next to the binary), e.g.
# after install.sh or `cargo build --release -p daedalus-cli -p daedalus-stub`.

set -eu
cd "$(dirname "$0")/.."   # catalog build commands are repo-root-relative

want="${1:-}"
mkdir -p hub/dist

python3 - "$want" <<'EOF'
import json, subprocess, sys

with open("hub/catalog.json") as f:
    catalog = json.load(f)

want = sys.argv[1] if len(sys.argv) > 1 else None
for app in catalog["apps"]:
    if want and app["id"] != want:
        continue
    if not app.get("buildable"):
        if not want:
            print("skip (recipe only):", app["id"])
        continue
    build = app["build"]
    print("building:", app["id"])
    subprocess.run(build, check=True)
    print("    done:", build[build.index("-o") + 1])
EOF

if [ -n "$want" ]; then
    echo "built hub/dist/$want.de" >&2
fi