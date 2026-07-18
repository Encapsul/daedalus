#!/usr/bin/env bash
# scripts/release.sh — create a release tag and push to trigger CI.
#
# Usage:
#   ./scripts/release.sh 0.1.0        # release v0.1.0
#   ./scripts/release.sh 0.2.0-rc1    # pre-release
set -euo pipefail

if [ $# -ne 1 ]; then
    echo "Usage: $0 <version>"
    echo "  e.g. $0 0.1.0"
    exit 1
fi

VERSION="$1"
TAG="v${VERSION}"

# Ensure clean working tree.
if [ -n "$(git status --porcelain)" ]; then
    echo "error: working tree is not clean. Commit or stash first."
    exit 1
fi

# Ensure we're on main.
BRANCH="$(git rev-parse --abbrev-ref HEAD)"
if [ "$BRANCH" != "main" ]; then
    echo "warning: you're on branch '$BRANCH', not 'main'. Continue? [y/N]"
    read -r REPLY
    if [ "$REPLY" != "y" ] && [ "$REPLY" != "Y" ]; then
        exit 1
    fi
fi

# Ensure tag doesn't already exist.
if git rev-parse "$TAG" >/dev/null 2>&1; then
    echo "error: tag $TAG already exists."
    exit 1
fi

echo "Creating release $TAG ..."
git tag -a "$TAG" -m "Release $TAG"
git push origin "$TAG"

echo ""
echo "Pushed $TAG. GitHub Actions will now:"
echo "  1. Build xbin-stub + xbin-crypto for linux-x64, linux-arm64, macos-arm64, macos-x64"
echo "  2. Create a GitHub Release with binaries and checksums"
echo ""
echo "Track progress: https://github.com/Tednoob17/x.bin/actions"
