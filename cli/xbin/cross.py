"""Cross-compilation support — resolves target-arch Python interpreters.

Downloads python-build-standalone distributions for the target architecture
and returns paths usable by the build pipeline.

The vendored Python is self-contained: stdlib, shared libraries, and
bundled extensions (_ssl, _hashlib, _ctypes, etc.) are all included.
The build pipeline does NOT need to resolve .so dependencies for
cross-build Python binaries.
"""

from __future__ import annotations

import os
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
from pathlib import Path

# python-build-standalone release dates per target arch.
# Using "install_only" tarballs which bundle everything Python needs.
_RELEASES: dict[str, str] = {
    "aarch64": "20241016",
    "x86_64": "20241016",
}

_BASE_URL = "https://github.com/astral-sh/python-build-standalone/releases/download"

# PEP 508 platform tags for pip --platform
# Maps our arch names to the manylinux tags used by PyPI wheels.
_PIP_PLATFORM: dict[str, str] = {
    "aarch64": "manylinux2014_aarch64",
    "x86_64": "manylinux2014_x86_64",
}


def _cache_dir() -> Path:
    """Shared cache dir for cross-compilation downloads."""
    base = os.environ.get("XDG_CACHE_HOME")
    d = (Path(base) if base else Path.home() / ".cache") / "xbin" / "cross"
    d.mkdir(parents=True, exist_ok=True)
    return d


def _python_version_tag() -> str:
    """Current Python version as '3.12'."""
    return f"{sys.version_info.major}.{sys.version_info.minor}"


def _tarball_name(target_arch: str, release_date: str) -> str:
    """Build the tarball filename for a target arch."""
    triples = {
        "aarch64": "aarch64-unknown-linux-gnu",
        "x86_64": "x86_64-unknown-linux-gnu",
    }
    triple = triples[target_arch]
    py_ver = (
        f"{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}"
    )
    return f"cpython-{py_ver}+{release_date}-{triple}-install_only.tar.gz"


def _download_url(url: str, dest: Path) -> None:
    """Download a URL to a local file."""
    print(f"  cross: downloading {url}", file=sys.stderr)
    urllib.request.urlretrieve(url, dest)


def host_arch() -> str:
    """Return the host architecture as a platform.machine() string."""
    return platform.machine()


def is_cross_build(target: str | None) -> bool:
    """Return True if target differs from host architecture."""
    if target is None:
        return False
    return target != host_arch()


def pip_download_target(
    requirements_file: Path,
    dest_dir: Path,
    target_arch: str,
    verbose: bool,
) -> None:
    """Download pure-Python + manylinux wheels for *target_arch* using host pip.

    Uses `pip download --only-binary=:all: --platform <tag>` to fetch
    target-arch wheels without needing QEMU or a foreign interpreter.
    Pure-Python wheels (no-arch / py3-none-any) are always compatible.
    Manylinux wheels matching the target arch tag are selected by pip.

    Raises RuntimeError if pip fails.
    """
    platform_tag = _PIP_PLATFORM.get(target_arch)
    if platform_tag is None:
        raise ValueError(
            f"no pip platform tag for target arch '{target_arch}'; "
            f"supported: {', '.join(sorted(_PIP_PLATFORM))}"
        )

    py_ver = f"{sys.version_info.major}.{sys.version_info.minor}"
    cmd = [
        sys.executable,
        "-m",
        "pip",
        "download",
        "--only-binary=:all:",
        "--platform",
        platform_tag,
        "--python-version",
        py_ver,
        "--no-deps",
        "-r",
        str(requirements_file),
        "--dest",
        str(dest_dir),
        "--quiet",
    ]
    if verbose:
        print(f"  pip (target={target_arch}): {' '.join(cmd)}", file=sys.stderr)
    result = subprocess.run(cmd, capture_output=True)
    if result.returncode != 0:
        stderr = result.stderr.decode(errors="replace").strip()
        raise RuntimeError(
            f"pip download for {target_arch} failed (exit {result.returncode}): {stderr}"
        )


def _vendored_python_version(vendored_root: Path) -> str | None:
    """Detect the Python version (X.Y) from a vendored python-build-standalone root."""
    lib = vendored_root / "lib"
    if lib.is_dir():
        for p in lib.iterdir():
            m = re.match(r"^python(\d+\.\d+)$", p.name)
            if m:
                return m.group(1)
    return None


def resolve_cross_python(target_arch: str) -> Path:
    """Download and extract a target-arch Python distribution.

    Returns the path to the extracted Python root directory, which contains
    bin/python3, lib/pythonX.Y/, and bundled shared libraries.

    The vendored Python is self-contained — no external .so resolution needed.
    Caches the download in ~/.cache/xbin/cross/{arch}/ for subsequent builds.

    Override with XBIN_CROSS_PYTHON env var to use a pre-extracted directory.

    Raises FileNotFoundError if the target architecture is not supported.
    Raises RuntimeError if the download or extraction fails.
    """
    override = os.environ.get("XBIN_CROSS_PYTHON")
    if override:
        p = Path(override)
        if p.is_dir() and (p / "bin" / "python3").exists():
            return p
        raise FileNotFoundError(
            f"XBIN_CROSS_PYTHON={override} does not contain bin/python3"
        )

    if target_arch not in _RELEASES:
        raise FileNotFoundError(
            f"cross-compilation not supported for '{target_arch}'; "
            f"supported: {', '.join(sorted(_RELEASES))}"
        )

    release_date = _RELEASES[target_arch]
    ver_tag = _python_version_tag()
    cache = _cache_dir() / target_arch / ver_tag
    marker = cache / ".extracted"
    if marker.exists():
        return cache

    tarball_name = _tarball_name(target_arch, release_date)
    url = f"{_BASE_URL}/{release_date}/{tarball_name}"

    with tempfile.TemporaryDirectory(prefix="xbin-cross-") as tmp:
        tarball_path = Path(tmp) / tarball_name
        try:
            _download_url(url, tarball_path)
        except OSError as e:
            raise RuntimeError(f"failed to download {url}: {e}") from e

        cache.parent.mkdir(parents=True, exist_ok=True)
        try:
            with tarfile.open(tarball_path, "r:gz") as tf:
                tf.extractall(cache)
            marker.write_text(f"{url}\n")
        except tarfile.TarError as e:
            shutil.rmtree(cache, ignore_errors=True)
            raise RuntimeError(f"failed to extract {tarball_name}: {e}") from e

    return cache


_DENO_BASE_URL = "https://github.com/denoland/deno/releases/latest/download"


def download_vendored_deno(target_arch: str = "x86_64") -> Path:
    """Download and cache a vendored Deno binary for *target_arch*.

    Returns the path to the cached `deno` binary.
    Caches in ~/.cache/xbin/cross/deno/{arch}/deno.

    Override with XBIN_CROSS_DENO env var to use a pre-existing binary.
    """
    override = os.environ.get("XBIN_CROSS_DENO")
    if override:
        p = Path(override)
        if p.is_file():
            return p
        raise FileNotFoundError(f"XBIN_CROSS_DENO={override} is not a file")

    arch_map = {
        "x86_64": "x86_64-unknown-linux-gnu",
        "aarch64": "aarch64-unknown-linux-gnu",
    }
    triple = arch_map.get(target_arch)
    if triple is None:
        raise FileNotFoundError(
            f"no vendored Deno for '{target_arch}'; "
            f"supported: {', '.join(sorted(arch_map))}"
        )

    cache = _cache_dir() / "deno" / target_arch
    binary = cache / "deno"
    if binary.exists():
        return binary

    zip_name = f"deno-{triple}.zip"
    url = f"{_DENO_BASE_URL}/{zip_name}"
    cache.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="xbin-deno-") as tmp:
        zip_path = Path(tmp) / zip_name
        try:
            _download_url(url, zip_path)
        except OSError as e:
            raise RuntimeError(f"failed to download Deno: {e}") from e

        import zipfile

        try:
            with zipfile.ZipFile(zip_path) as zf:
                zf.extractall(cache)
        except zipfile.BadZipFile as e:
            shutil.rmtree(cache, ignore_errors=True)
            raise RuntimeError(f"failed to extract {zip_name}: {e}") from e

    if not binary.exists():
        raise RuntimeError(f"Deno binary not found after extracting {zip_name}")

    return binary


def cross_python_root(cache_dir: Path) -> Path:
    """Given a cache dir from resolve_cross_python, return the Python root.

    The install_only tarball extracts into a top-level 'python/' directory.
    """
    root = cache_dir / "python"
    if root.is_dir():
        return root
    if (cache_dir / "bin" / "python3").exists():
        return cache_dir
    raise FileNotFoundError(
        f"vendored Python cache at {cache_dir} has unexpected structure"
    )
