#!/usr/bin/env bash
# install.sh — install or upgrade daedalus
# Usage: curl -fsSL https://raw.githubusercontent.com/Tednoob17/daedalus/main/scripts/install.sh | bash
set -euo pipefail

REPO="Tednoob17/daedalus"
INSTALL_DIR="${DAEDALUS_INSTALL_DIR:-/usr/local/bin}"
GITHUB="https://github.com/${REPO}"

# ── Helpers ────────────────────────────────────────────────────────────
info()  { printf "\033[1;34m[daedalus]\033[0m %s\n" "$*"; }
ok()    { printf "\033[1;32m[daedalus]\033[0m %s\n" "$*"; }
warn()  { printf "\033[1;33m[daedalus]\033[0m %s\n" "$*"; }
err()   { printf "\033[1;31m[daedalus]\033[0m %s\n" "$*" >&2; exit 1; }

detect_platform() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Linux*)  os="linux" ;;
    Darwin*) os="macos" ;;
    *)       err "unsupported OS: $os" ;;
  esac

  case "$arch" in
    x86_64|amd64)   arch="x64" ;;
    aarch64|arm64)  arch="arm64" ;;
    *)              err "unsupported architecture: $arch" ;;
  esac

  echo "${os}-${arch}"
}

verify_checksum() {
  local file="$1" checksum_file="$2"

  if [ ! -f "$checksum_file" ]; then
    warn "no checksum file found, skipping verification"
    return 0
  fi

  local expected got
  expected="$(grep "$(basename "$file")" "$checksum_file" 2>/dev/null | awk '{print $1}')"

  if [ -z "$expected" ]; then
    warn "checksum for $(basename "$file") not found in checksums.txt, skipping"
    return 0
  fi

  if command -v sha256sum &>/dev/null; then
    got="$(sha256sum "$file" | awk '{print $1}')"
  elif command -v shasum &>/dev/null; then
    got="$(shasum -a 256 "$file" | awk '{print $1}')"
  else
    warn "no sha256sum or shasum found, skipping verification"
    return 0
  fi

  if [ "$expected" != "$got" ]; then
    err "checksum mismatch!\n  expected: ${expected}\n  got:      ${got}"
  fi
  ok "checksum verified"
}

# ── Main ───────────────────────────────────────────────────────────────
main() {
  local platform version tag url tmpdir

  platform="$(detect_platform)"
  info "detected platform: ${platform}"

  # Get latest version from GitHub API
  if command -v curl &>/dev/null; then
    version="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
      | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"v\?\([^"]*\)".*/\1/')"
  elif command -v wget &>/dev/null; then
    version="$(wget -qO- "https://api.github.com/repos/${REPO}/releases/latest" \
      | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"v\?\([^"]*\)".*/\1/')"
  else
    err "curl or wget required"
  fi

  if [ -z "$version" ]; then
    err "could not determine latest version"
  fi

  tag="v${version}"
  info "latest version: ${version}"

  # Check if already installed and up-to-date
  if command -v daedalus &>/dev/null; then
    local current
    current="$(daedalus --version 2>/dev/null | head -1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' || echo "")"
    if [ "$current" = "$version" ]; then
      ok "daedalus ${version} is already installed"
      return 0
    fi
    [ -n "$current" ] && info "upgrading from ${current} to ${version}"
  fi

  # Download
  tmpdir="$(mktemp -d)"
  trap 'rm -rf "${tmpdir}"' EXIT

  asset="daedalus-${platform}.tar.gz"
  url="${GITHUB}/releases/download/${tag}/${asset}"

  info "downloading ${asset}..."
  if command -v curl &>/dev/null; then
    curl -fsSL "$url" -o "${tmpdir}/${asset}" \
      || err "download failed — is version ${version} available for ${platform}?"
  else
    wget -q "$url" -O "${tmpdir}/${asset}" \
      || err "download failed — is version ${version} available for ${platform}?"
  fi

  # Download checksums.txt
  local checksum_url="${GITHUB}/releases/download/${tag}/checksums.txt"
  if command -v curl &>/dev/null; then
    curl -fsSL "$checksum_url" -o "${tmpdir}/checksums.txt" 2>/dev/null || true
  else
    wget -q "$checksum_url" -O "${tmpdir}/checksums.txt" 2>/dev/null || true
  fi

  # Verify checksum
  verify_checksum "${tmpdir}/${asset}" "${tmpdir}/checksums.txt"

  # Extract
  info "extracting..."
  tar xzf "${tmpdir}/${asset}" -C "${tmpdir}"

  local extracted_dir="${tmpdir}/daedalus-${platform}"
  if [ ! -d "$extracted_dir" ]; then
    extracted_dir="$(find "${tmpdir}" -maxdepth 1 -type d -name 'daedalus-*' | head -1)"
  fi

  if [ ! -d "$extracted_dir" ]; then
    err "unexpected archive structure — no directory found"
  fi

  # Install binaries
  info "installing to ${INSTALL_DIR}..."
  if [ -w "$INSTALL_DIR" ] 2>/dev/null; then
    cp "${extracted_dir}/"* "$INSTALL_DIR/"
  else
    sudo cp "${extracted_dir}/"* "$INSTALL_DIR/"
  fi

  ok "installed daedalus ${version} to ${INSTALL_DIR}"
  info "run 'daedalus --version' to verify"
}

main "$@"
