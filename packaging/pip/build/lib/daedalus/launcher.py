# pip release launcher for daedalus.
#
# pip/yarn-style native binaries cannot be shipped as wheels from a Rust
# release flow, so this package installs a thin console script that resolves
# the matching GitHub release tarball (checksum-verified) into a user cache
# and execs the real daedalus binary. Every run is a cache hit after the
# first download. See `daedalus-cli/src/release.rs` for the same logic in the
# Rust CLI.
"""daedalus release launcher."""

import hashlib
import io
import os
import platform
import stat
import sys
import tarfile
import urllib.request
from dataclasses import dataclass
from pathlib import Path

VERSION = "0.7.1"
REPO = "Encapsul/daedalus"
CACHE_DIR = Path.home() / ".cache" / "daedalus" / "pip"
MAX_TARBALL_BYTES = 200 * 1024 * 1024


@dataclass
class Platform:
    os: str
    arch: str


def detect_platform() -> Platform:
    """Map the running host to the (os, arch) tags used in release asset names."""
    system = platform.system().lower()
    machine = platform.machine().lower()

    os_map = {"linux": "linux", "darwin": "darwin", "windows": "windows"}
    if system not in os_map:
        raise SystemExit(f"daedalus: unsupported platform: {platform.system()}")

    if machine in ("x86_64", "amd64"):
        arch = "amd64"
    elif machine in ("aarch64", "arm64"):
        arch = "arm64"
    else:
        raise SystemExit(f"daedalus: unsupported architecture: {platform.machine()}")

    return Platform(os_map[system], arch)


def release_url(p: Platform) -> str:
    """GitHub release download URL for this platform's asset."""
    asset = f"daedalus_{VERSION}_{p.os}_{p.arch}.tar.gz"
    return f"https://github.com/{REPO}/releases/download/v{VERSION}/{asset}"


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def fetch_text(url: str) -> str:
    with urllib.request.urlopen(url, timeout=30) as resp:
        return resp.read().decode("utf-8")


def expected_checksum(p: Platform) -> str:
    """Fetch the sha256 for this asset from the release checksums.txt."""
    checksums_url = f"https://github.com/{REPO}/releases/download/v{VERSION}/checksums.txt"
    asset = f"daedalus_{VERSION}_{p.os}_{p.arch}.tar.gz"
    return parse_checksums(fetch_text(checksums_url), asset)


def parse_checksums(text: str, asset: str) -> str:
    """Return the sha256 for `asset` from a release checksums.txt body."""
    for line in text.splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[1].lstrip("*") == asset:
            return parts[0]
    raise SystemExit(f"daedalus: no checksum for {asset} in release checksums.txt")


def download_verified(url: str, expected: str) -> bytes:
    """Download a release asset, rejecting oversized or unverified payloads."""
    with urllib.request.urlopen(url, timeout=600) as resp:
        data = resp.read()
    if len(data) > MAX_TARBALL_BYTES:
        raise SystemExit(f"daedalus: release asset too large: {len(data)} bytes")
    got = sha256_bytes(data)
    if got != expected:
        raise SystemExit(
            f"daedalus: checksum mismatch for {url}: expected {expected}, got {got}"
        )
    return data


def extract_daedalus(data: bytes, dest: Path) -> Path:
    """Unpack the release tarball (daedalus + stub + crypto) into dest.

    The Rust CLI resolves `daedalus-stub` next to the `daedalus` binary, so
    all three release binaries are materialised in the cache directory."""
    dest.mkdir(parents=True, exist_ok=True)
    extracted = None
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
        for member in tar.getmembers():
            name = Path(member.name)
            if ".." in name.parts:
                raise SystemExit(f"daedalus: unsafe path in archive: {member.name}")
            base = name.name
            if not name.parent.name.startswith("daedalus_"):
                continue
            stem = base[:-4] if base.endswith(".exe") else base
            if stem not in ("daedalus", "daedalus-stub", "daedalus-crypto"):
                continue
            if not member.isfile():
                continue
            f = tar.extractfile(member)
            if f is None:
                continue
            target = dest / base
            target.write_bytes(f.read())
            if os.name != "nt":
                target.chmod(target.stat().st_mode | stat.S_IXUSR)
            if stem == "daedalus":
                extracted = target
    if extracted is None:
        raise SystemExit("daedalus: did not find daedalus binary in release archive")
    return extracted


def cache_dir() -> Path:
    return CACHE_DIR / VERSION / f"{detect_platform().os}_{detect_platform().arch}"


def resolve_binary() -> Path:
    """Return the cached daedalus binary, downloading it on first use."""
    p = detect_platform()
    where = cache_dir()
    binary = where / "daedalus.exe" if os.name == "nt" else where / "daedalus"
    if binary.exists():
        return binary

    url = release_url(p)
    sys.stderr.write(f"[daedalus] downloading {url}...\n")
    expected = expected_checksum(p)
    data = download_verified(url, expected)
    extracted = extract_daedalus(data, where)
    return extracted


def main() -> int:
    binary = resolve_binary()
    os.execv(str(binary), [str(binary)] + sys.argv[1:])


if __name__ == "__main__":
    sys.exit(main())