#!/bin/sh
# Deprecated entry point. Prefer the repository-root installer:
#
#   curl -fsSL https://raw.githubusercontent.com/Encapsul/daedalus/main/install.sh | sh
#
# That script installs into ~/.local/bin, checks sha256 against checksums.txt,
# and refuses to continue when the check cannot run. This wrapper keeps old
# links working by forwarding to it.

set -eu

echo "daedalus: scripts/install.sh is deprecated; forwarding to install.sh" >&2

here=$(CDPATH= cd -- "$(dirname "$0")" 2>/dev/null && pwd) || here=""
if [ -n "$here" ] && [ -f "$here/../install.sh" ]; then
    exec sh "$here/../install.sh" "$@"
fi

curl -fsSL https://raw.githubusercontent.com/Encapsul/daedalus/main/install.sh | sh
