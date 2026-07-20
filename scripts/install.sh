#!/usr/bin/env bash
# install.sh — install or upgrade x.bin
# Usage: curl -fsSL https://raw.githubusercontent.com/Tednoob17/x.bin/main/scripts/install.sh | bash
set -euo pipefail

REPO="Tednoob17/x.bin"
INSTALL_DIR="${XBIN_INSTALL_DIR:-/usr/local/bin}"
GITHUB="https://github.com/${REPO}"

# ── Helpers ────────────────────────────────────────────────────────────
info()  { printf "\033[1;34m[xbin]\033[0m %s\n" "$*"; }
ok()    { printf "\033[1;32m[xbin]\033[0m %s\n" "$*"; }
err()   { printf "\033[1;31m[xbin]\033[0m %s\n" "$*" >&2; exit 1; }

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
  if command -v xbin &>/dev/null; then
    local current
    current="$(xbin --version 2>/dev/null | head -1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' || echo "")"
    if [ "$current" = "$version" ]; then
      ok "xbin ${version} is already installed"
      return 0
    fi
    [ -n "$current" ] && info "upgrading from ${current} to ${version}"
  fi

  # Download
  tmpdir="$(mktemp -d)"
  trap 'rm -rf "$tmpdir"' EXIT

  local asset="xbin-${version}-${platform}.tar.gz"
  url="${GITHUB}/releases/download/${tag}/${asset}"

  info "downloading ${asset}..."
  if command -v curl &>/dev/null; then
    curl -fsSL "$url" -o "${tmpdir}/${asset}" \
      || err "download failed — is version ${version} available for ${platform}?"
  else
    wget -q "$url" -O "${tmpdir}/${asset}" \
      || err "download failed — is version ${version} available for ${platform}?"
  fi

  # Verify checksum
  local checksum_url="${GITHUB}/releases/download/${tag}/${asset}.sha256"
  if command -v curl &>/dev/null; then
    curl -fsSL "$checksum_url" -o "${tmpdir}/${asset}.sha256" 2>/dev/null || true
  else
    wget -q "$checksum_url" -O "${tmpdir}/${asset}.sha256" 2>/dev/null || true
  fi

  if [ -f "${tmpdir}/${asset}.sha256" ] && command -v sha256sum &>/dev/null; then
    local expected got
    expected="$(awk '{print $1}' "${tmpdir}/${asset}.sha256")"
    got="$(sha256sum "${tmpdir}/${asset}" | awk '{print $1}')"
    if [ "$expected" != "$got" ]; then
      err "checksum mismatch: expected ${expected}, got ${got}"
    fi
    ok "checksum verified"
  elif [ -f "${tmpdir}/${asset}.sha256" ] && command -v shasum &>/dev/null; then
    local expected got
    expected="$(awk '{print $1}' "${tmpdir}/${asset}.sha256")"
    got="$(shasum -a 256 "${tmpdir}/${asset}" | awk '{print $1}')"
    if [ "$expected" != "$got" ]; then
      err "checksum mismatch: expected ${expected}, got ${got}"
    fi
    ok "checksum verified"
  fi

  # Extract
  info "extracting..."
  tar xzf "${tmpdir}/${asset}" -C "${tmpdir}"

  local extracted_dir="${tmpdir}/xbin-${version}-${platform}"
  if [ ! -d "$extracted_dir" ]; then
    # Try without version in dir name
    extracted_dir="$(find "${tmpdir}" -maxdepth 1 -type d -name 'xbin-*' | head -1)"
  fi

  if [ ! -d "$extracted_dir/bin" ]; then
    err "unexpected archive structure — no bin/ directory found"
  fi

  # Install
  info "installing to ${INSTALL_DIR}..."
  if [ -w "$INSTALL_DIR" ] 2>/dev/null; then
    cp "${extracted_dir}/bin/"* "$INSTALL_DIR/"
  else
    sudo cp "${extracted_dir}/bin/"* "$INSTALL_DIR/"
  fi

  ok "installed xbin ${version} to ${INSTALL_DIR}"
  info "run 'xbin --version' to verify"
}

main "$@"
