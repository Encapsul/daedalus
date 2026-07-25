"""Auto-download missing runtime binaries into a local cache.

Runtimes are fetched on-demand when the build machine does not have them
installed.  Downloaded binaries are cached under ``~/.cache/xbin/runtimes/``
and reused across builds.

Supported runtimes:
  - node (Node.js LTS from nodejs.org)
  - php (static build from GitHub releases)
"""
from __future__ import annotations

import hashlib
import os
import platform
import shutil
import tarfile
import urllib.request
import zipfile
from pathlib import Path

# ---------------------------------------------------------------------------
# Cache directory
# ---------------------------------------------------------------------------

_RUNTIME_CACHE: Path | None = None


def runtime_cache_dir() -> Path:
    global _RUNTIME_CACHE
    if _RUNTIME_CACHE is None:
        base = os.environ.get("XDG_CACHE_HOME")
        _RUNTIME_CACHE = (
            (Path(base) if base else Path.home() / ".cache") / "xbin" / "runtimes"
        )
        _RUNTIME_CACHE.mkdir(parents=True, exist_ok=True)
    return _RUNTIME_CACHE


# ---------------------------------------------------------------------------
# Architecture helpers
# ---------------------------------------------------------------------------

_HOST_ARCH = platform.machine()


def _node_arch() -> str:
    mapping = {
        "x86_64": "x64",
        "aarch64": "arm64",
        "armv7l": "armv7l",
        "arm64": "arm64",
    }
    return mapping.get(_HOST_ARCH, "x64")


def _node_platform() -> str:
    if _HOST_ARCH in ("aarch64", "arm64"):
        return "linux-arm64"
    return "linux-x64"


# ---------------------------------------------------------------------------
# Download helpers
# ---------------------------------------------------------------------------

def _download(url: str, dest: Path, verbose: bool = True) -> None:
    if verbose:
        print(f"  [xbin] downloading {url}", file=__import__("sys").stderr)
    try:
        urllib.request.urlretrieve(url, str(dest))
    except Exception as e:
        raise RuntimeError(f"failed to download {url}: {e}") from e


def _extract_tar_xz(archive: Path, dest: Path, verbose: bool = True) -> None:
    with tarfile.open(archive, "r:xz") as tf:
        tf.extractall(path=str(dest))
    if verbose:
        print(f"  [xbin] extracted {archive.name}", file=__import__("sys").stderr)


def _extract_zip(archive: Path, dest: Path, verbose: bool = True) -> None:
    with zipfile.ZipFile(archive, "r") as zf:
        zf.extractall(path=str(dest))
    if verbose:
        print(f"  [xbin] extracted {archive.name}", file=__import__("sys").stderr)


def _hash_file(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


# ---------------------------------------------------------------------------
# Node.js downloader
# ---------------------------------------------------------------------------

_NODE_LTS_VERSION = "v24.18.0"


def download_node(verbose: bool = True) -> Path:
    """Download Node.js LTS if not already cached.

    Returns the path to the ``node`` binary inside the extracted directory.
    """
    cache = runtime_cache_dir() / "node"
    cache.mkdir(parents=True, exist_ok=True)

    platform_name = _node_platform()
    version = _NODE_LTS_VERSION
    base_name = f"node-{version}-{platform_name}"
    archive_name = f"{base_name}.tar.xz"
    url = f"https://nodejs.org/dist/{version}/{archive_name}"

    # Check if already extracted
    expected_bin = cache / base_name / "bin" / "node"
    if expected_bin.is_file():
        if verbose:
            print(f"  [xbin] using cached Node.js at {expected_bin}", file=__import__("sys").stderr)
        return expected_bin

    archive_path = cache / archive_name
    extract_dir = cache / base_name

    if not archive_path.is_file():
        _download(url, archive_path, verbose=verbose)

    if not extract_dir.is_dir():
        _extract_tar_xz(archive_path, cache, verbose=verbose)

    if expected_bin.is_file():
        if verbose:
            print(f"  [xbin] Node.js ready at {expected_bin}", file=__import__("sys").stderr)
        return expected_bin

    raise RuntimeError(f"Node.js binary not found after extraction at {expected_bin}")


# ---------------------------------------------------------------------------
# PHP downloader (static build)
# ---------------------------------------------------------------------------

_PHP_VERSION = "8.3.20"


def _find_nvm_node() -> Path | None:
    """Try to locate Node.js in common NVM paths."""
    home = Path.home()
    nvm_dir = home / ".nvm" / "versions" / "node"
    if not nvm_dir.is_dir():
        return None
    # Pick the latest version directory
    versions = sorted(nvm_dir.iterdir(), reverse=True)
    for vdir in versions:
        node_bin = vdir / "bin" / "node"
        if node_bin.is_file():
            return node_bin
    return None


def find_node(verbose: bool = True) -> Path:
    """Locate Node.js, downloading it if necessary."""
    node = shutil.which("node")
    if node:
        return Path(node).resolve()

    nvm_node = _find_nvm_node()
    if nvm_node:
        if verbose:
            print(f"  [xbin] found Node.js in NVM: {nvm_node}", file=__import__("sys").stderr)
        return nvm_node

    if verbose:
        print("  [xbin] Node.js not found, downloading...", file=__import__("sys").stderr)
    return download_node(verbose=verbose)


# ---------------------------------------------------------------------------
# PHP downloader (static build from shivammathur/php-binary)
# ---------------------------------------------------------------------------

def download_php(verbose: bool = True) -> Path:
    """Download a static PHP binary if not already cached.

    Returns the path to the ``php`` binary.
    """
    cache = runtime_cache_dir() / "php"
    cache.mkdir(parents=True, exist_ok=True)

    version = _PHP_VERSION
    arch = "x64" if _HOST_ARCH in ("x86_64",) else "arm64"
    binary_name = f"php-{version}-linux-{arch}"
    binary_path = cache / binary_name / "usr" / "local" / "bin" / "php"

    if binary_path.is_file():
        if verbose:
            print(f"  [xbin] using cached PHP at {binary_path}", file=__import__("sys").stderr)
        return binary_path

    url = f"https://github.com/shivammathur/php-binary/releases/download/{version}/{binary_name}.tar.xz"
    archive_path = cache / f"{binary_name}.tar.xz"

    if not archive_path.is_file():
        _download(url, archive_path, verbose=verbose)

    extract_dir = cache / binary_name
    if not extract_dir.is_dir():
        _extract_tar_xz(archive_path, cache, verbose=verbose)

    if binary_path.is_file():
        binary_path.chmod(0o755)
        if verbose:
            print(f"  [xbin] PHP ready at {binary_path}", file=__import__("sys").stderr)
        return binary_path

    raise RuntimeError(f"PHP binary not found after extraction at {binary_path}")


def find_php(verbose: bool = True) -> Path:
    """Locate PHP, downloading it if necessary."""
    php = shutil.which("php")
    if php:
        return Path(php).resolve()

    if verbose:
        print("  [xbin] PHP not found, downloading static build...", file=__import__("sys").stderr)
    return download_php(verbose=verbose)


# ---------------------------------------------------------------------------
# Registry of auto-downloadable runtimes
# ---------------------------------------------------------------------------

RUNTIME_FINDERS: dict[str, callable] = {
    "node": find_node,
    "php": find_php,
}


def find_runtime(name: str, verbose: bool = True) -> Path | None:
    """Find a runtime binary, auto-downloading if supported and missing."""
    finder = RUNTIME_FINDERS.get(name)
    if finder is None:
        return None
    try:
        return finder(verbose=verbose)
    except Exception as e:
        if verbose:
            print(f"  [xbin] warning: auto-download of {name} failed: {e}", file=__import__("sys").stderr)
        return None
