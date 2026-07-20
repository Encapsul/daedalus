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
warn()  { printf "\033[1;33m[xbin]\033[0m %s\n" "$*"; }
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

verify_checksum() {
  local file="$1" checksum_file="$2"

  if [ ! -f "$checksum_file" ]; then
    warn "no checksum file found, skipping verification"
    return 0
  fi

  local expected got
  expected="$(grep "$(basename "$file")" "$checksum_file" 2>/dev/null | awk '{print $1}')"

  if [ -z "$expected" ]; then
    warn "checksum for $(basename "$file") not found in SHASUMS256.txt, skipping"
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
  trap 'rm -rf "${tmpdir}"' EXIT

  asset="xbin-${platform}.tar.gz"
  url="${GITHUB}/releases/download/${tag}/${asset}"

  info "downloading ${asset}..."
  if command -v curl &>/dev/null; then
    curl -fsSL "$url" -o "${tmpdir}/${asset}" \
      || err "download failed — is version ${version} available for ${platform}?"
  else
    wget -q "$url" -O "${tmpdir}/${asset}" \
      || err "download failed — is version ${version} available for ${platform}?"
  fi

  # Download SHASUMS256.txt
  local checksum_url="${GITHUB}/releases/download/${tag}/SHASUMS256.txt"
  if command -v curl &>/dev/null; then
    curl -fsSL "$checksum_url" -o "${tmpdir}/SHASUMS256.txt" 2>/dev/null || true
  else
    wget -q "$checksum_url" -O "${tmpdir}/SHASUMS256.txt" 2>/dev/null || true
  fi

  # Verify checksum
  verify_checksum "${tmpdir}/${asset}" "${tmpdir}/SHASUMS256.txt"

  # Extract
  info "extracting..."
  tar xzf "${tmpdir}/${asset}" -C "${tmpdir}"

  local extracted_dir="${tmpdir}/xbin-${platform}"
  if [ ! -d "$extracted_dir" ]; then
    extracted_dir="$(find "${tmpdir}" -maxdepth 1 -type d -name 'xbin-*' | head -1)"
  fi

  if [ ! -d "$extracted_dir/bin" ]; then
    err "unexpected archive structure — no bin/ directory found"
  fi

  # Install binaries
  info "installing to ${INSTALL_DIR}..."
  if [ -w "$INSTALL_DIR" ] 2>/dev/null; then
    cp "${extracted_dir}/bin/"* "$INSTALL_DIR/"
  else
    sudo cp "${extracted_dir}/bin/"* "$INSTALL_DIR/"
  fi

  # Install Python CLI lib if present
  if [ -d "$extracted_dir/lib/python/xbin" ]; then
    local xbin_lib="${INSTALL_DIR}/../lib/xbin/python"
    info "installing Python CLI to ${xbin_lib}..."
    mkdir -p "$xbin_lib"
    if [ -w "$(dirname "$xbin_lib")" ] 2>/dev/null; then
      cp -r "${extracted_dir}/lib/python/xbin" "$xbin_lib/"
    else
      sudo cp -r "${extracted_dir}/lib/python/xbin" "$xbin_lib/"
    fi
    # Update wrapper script with correct lib path
    local wrapper="$INSTALL_DIR/xbin"
    if [ -f "$wrapper" ]; then
      sed -i "s|/lib/python|/../lib/xbin/python|g" "$wrapper" 2>/dev/null || true
    fi
  fi

  ok "installed xbin ${version} to ${INSTALL_DIR}"
  info "run 'xbin --version' to verify"
}

main "$@"
