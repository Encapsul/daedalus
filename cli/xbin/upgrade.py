"""xbin upgrade: self-update x.bin to the latest release."""

from __future__ import annotations

import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from . import __version__ as _XBIN_VERSION
from ._color import green, red

_GITHUB_API = "https://api.github.com/repos/Tednoob17/x.bin/releases/latest"


def _detect_platform() -> str:
    os_name = platform.system().lower()
    arch = platform.machine().lower()
    if os_name == "linux":
        os_name = "linux"
    elif os_name == "darwin":
        os_name = "macos"
    else:
        raise RuntimeError(f"unsupported OS: {platform.system()}")
    if arch in ("x86_64", "amd64"):
        arch = "x64"
    elif arch in ("aarch64", "arm64"):
        arch = "arm64"
    else:
        raise RuntimeError(f"unsupported architecture: {arch}")
    return f"{os_name}-{arch}"


def _fetch_latest_version() -> str:
    """Get latest version from GitHub API."""
    try:
        import urllib.request

        req = urllib.request.Request(
            _GITHUB_API, headers={"Accept": "application/vnd.github.v3+json"}
        )
        with urllib.request.urlopen(req, timeout=10) as resp:
            data = json.loads(resp.read())
            tag = data.get("tag_name", "")
            return tag.lstrip("v")
    except Exception as e:
        raise RuntimeError(f"failed to fetch latest version: {e}") from e


def _find_xbin_binary() -> Path:
    """Locate the running xbin binary."""
    # Try /proc/self/exe (Linux)
    proc = Path("/proc/self/exe")
    if proc.exists():
        return Path(os.readlink(str(proc)))

    # Try argv[0]
    if sys.argv[0] and os.path.isfile(sys.argv[0]):
        return Path(sys.argv[0]).resolve()

    # Try PATH
    for p in os.environ.get("PATH", "").split(os.pathsep):
        candidate = Path(p) / "xbin"
        if candidate.is_file():
            return candidate

    raise FileNotFoundError("cannot locate xbin binary for self-update")


def upgrade(*, verbose: bool = True) -> None:
    """Upgrade x.bin to the latest release."""
    current = _XBIN_VERSION
    if verbose:
        print(f"[xbin] current version: {current}", file=sys.stderr)

    try:
        latest = _fetch_latest_version()
    except RuntimeError as e:
        print(f"[xbin] {red(str(e))}", file=sys.stderr)
        sys.exit(1)

    if verbose:
        print(f"[xbin] latest version:  {latest}", file=sys.stderr)

    if current == latest:
        print("[xbin] already up to date", file=sys.stderr)
        return

    platform_str = _detect_platform()
    if verbose:
        print(f"[xbin] platform: {platform_str}", file=sys.stderr)

    tag = f"v{latest}"
    asset = f"xbin-{latest}-{platform_str}.tar.gz"
    url = f"https://github.com/Tednoob17/x.bin/releases/download/{tag}/{asset}"

    with tempfile.TemporaryDirectory(prefix="xbin-upgrade-") as tmp:
        tmp_path = Path(tmp)
        tarball = tmp_path / asset

        # Download
        if verbose:
            print(f"[xbin] downloading {asset}...", file=sys.stderr)
        try:
            import urllib.request

            urllib.request.urlretrieve(url, str(tarball))
        except Exception as e:
            print(f"[xbin] {red(f'download failed: {e}')}", file=sys.stderr)
            sys.exit(1)

        # Verify checksum
        checksum_url = f"{url}.sha256"
        try:
            import urllib.request

            req = urllib.request.Request(checksum_url)
            with urllib.request.urlopen(req, timeout=10) as resp:
                expected = resp.read().decode().strip().split()[0]

            import hashlib

            got = hashlib.sha256(tarball.read_bytes()).hexdigest()
            if expected != got:
                print(
                    f"[xbin] {red(f'checksum mismatch: expected {expected}, got {got}')}",
                    file=sys.stderr,
                )
                sys.exit(1)
            if verbose:
                print("[xbin] checksum verified", file=sys.stderr)
        except Exception:
            if verbose:
                print("[xbin] warning: could not verify checksum", file=sys.stderr)

        # Extract
        subprocess.run(["tar", "xzf", str(tarball), "-C", str(tmp_path)], check=True)

        # Find extracted dir
        extracted = None
        for d in tmp_path.iterdir():
            if d.is_dir() and d.name.startswith("xbin-"):
                extracted = d
                break
        if extracted is None:
            print("[xbin] error: unexpected archive structure", file=sys.stderr)
            sys.exit(1)

        bin_dir = extracted / "bin"
        if not bin_dir.is_dir():
            print("[xbin] error: no bin/ directory in archive", file=sys.stderr)
            sys.exit(1)

        # Find install location
        xbin_path = _find_xbin_binary()
        install_dir = xbin_path.parent

        if verbose:
            print(f"[xbin] installing to {install_dir}...", file=sys.stderr)

        for binary in bin_dir.iterdir():
            dest = install_dir / binary.name
            if os.access(dest, os.W_OK):
                shutil.copy2(str(binary), str(dest))
            else:
                subprocess.run(["sudo", "cp", str(binary), str(dest)], check=True)

    print(f"[xbin] {green(f'upgraded to {latest}')}", file=sys.stderr)
