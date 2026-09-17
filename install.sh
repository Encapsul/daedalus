#!/bin/sh
# daedalus installer — single-command install from GitHub Releases.
#
#   curl -fsSL https://raw.githubusercontent.com/Encapsul/daedalus/main/install.sh | sh
#
# Installs daedalus + daedalus-stub into ~/.local/bin (no sudo). Overridable:
#   DAEDALUS_VERSION        release tag (default: latest from GitHub API)
#   DAEDALUS_INSTALL_DIR    target directory (default: $HOME/.local/bin)
#   DAEDALUS_MIRROR         base URL (default: GitHub releases, for mirror testing)
#
# Security: the artifact is verified against the release checksums.txt
# (sha256) before installation. No code from the download runs until then.

set -eu

: "${DAEDALUS_INSTALL_DIR:=$HOME/.local/bin}"
: "${DAEDALUS_MIRROR:=https://github.com/Encapsul/daedalus/releases}"
VERSION="${DAEDALUS_VERSION:-}"

HTTP_FETCH=curl
if ! command -v curl >/dev/null 2>&1; then
    HTTP_FETCH="wget -qO-"
fi
fetch() { # fetch <url>
    if [ "$HTTP_FETCH" = curl ]; then
        curl -fsSL "$1"
    else
        wget -qO- "$1"
    fi
}

os="$(uname -s | tr '[:upper:]' '[:lower:]')"
arch="$(uname -m)"
case "$os" in
    linux)  os=linux ;;
    darwin) os=darwin ;;
    *)      echo "daedalus installer: unsupported OS: $os" >&2; exit 1 ;;
esac
case "$arch" in
    x86_64|amd64)       arch=amd64 ;;
    aarch64|arm64)      arch=arm64 ;;
    *)                  echo "daedalus installer: unsupported arch: $arch" >&2; exit 1 ;;
esac

release_url() { printf '%s' "$DAEDALUS_MIRROR"; }

if [ -z "$VERSION" ]; then
    # JSON from api.github.com avoids guessing the latest tag; the raw
    # "tag_name" is trusted only to build the download path (sha256 check
    # below is the actual trust anchor).
    VERSION=$(fetch "https://api.github.com/repos/Encapsul/daedalus/releases/latest" \
        | sed -n 's/.*"tag_name": *"\(v[^"]*\)".*/\1/p' | head -1)
    if [ -z "$VERSION" ]; then
        VERSION=v0.7.0
        echo "daedalus installer: could not resolve latest release, falling back to $VERSION" >&2
    fi
fi

asset="daedalus_${VERSION#v}_${os}_${arch}.tar.gz"
url="$(release_url)/download/$VERSION/$asset"
echo "daedalus installer: resolving $asset from $VERSION"
files_url="$(release_url)/download/$VERSION/checksums.txt"

trap 'rm -rf "$tmpdir"' EXIT INT TERM
tmpdir="$(mktemp -d 2>/dev/null || mktemp -d /tmp/daedalus.XXXXXX)"
archive="$tmpdir/$asset"

echo "daedalus installer: downloading $asset ..."
fetch "$url" > "$archive"

echo "daedalus installer: verifying sha256 ..."
expected="$(fetch "$files_url" | awk -v a="$asset" '$2 == a { print $1 }' | head -1)"
if [ -z "$expected" ]; then
    echo "daedalus installer: FATAL: $asset missing from release checksums.txt" >&2
    exit 1
fi
if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$archive" | awk '{ print $1 }')"
else
    actual="$(shasum -a 256 "$archive" | awk '{ print $1 }')"
fi
if [ "$actual" != "$expected" ]; then
    echo "daedalus installer: FATAL: sha256 mismatch ($actual)" >&2
    exit 1
fi

mkdir -p "$DAEDALUS_INSTALL_DIR"
tar -xzf "$archive" -C "$tmpdir"
dir_suffix="daedalus_${VERSION#v}_${os}_${arch}"
for bin in daedalus daedalus-stub daedalus-crypto; do
    if [ -f "$tmpdir/$dir_suffix/$bin" ]; then
        cp "$tmpdir/$dir_suffix/$bin" "$DAEDALUS_INSTALL_DIR/$bin"
        chmod +x "$DAEDALUS_INSTALL_DIR/$bin"
    fi
done

echo ""
echo "Installed daedalus ${VERSION} -> $DAEDALUS_INSTALL_DIR"
echo ""
echo "Add it to your PATH (already done if $DAEDALUS_INSTALL_DIR is on it):"
echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
echo ""
echo "Then package your first app (60 s, no install of anything else):"
echo "  daedalus build ./examples/offline-health-agri -o clinic-agent.de"
echo "  ./clinic-agent.de diagnose"