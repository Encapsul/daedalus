"""Fetch detected dependencies into an isolated staging directory.

Never touches the real system: no ``apt-get install``, no global ``pip``/
``npm`` install.  Each fetcher writes into ``~/.cache/xbin/stage/{key}/``
and records what was fetched for auditability.

Checksums are recorded (SHA-256 of each downloaded file) but NOT verified
against upstream — most detected deps have no project-provided checksum.
The manifest is an audit log, not a trust anchor.
"""

from __future__ import annotations

import contextlib
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tarfile
import urllib.request
from dataclasses import dataclass
from pathlib import Path

from .dockerfile import DetectedDep

# ---------------------------------------------------------------------------
# Data structures
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class FetchResult:
    """Result of fetching a single dependency.

    Attributes:
        dep: The original detected dependency.
        ok: Whether the fetch succeeded.
        error: Human-readable failure reason (None on success).
        sha256: Hex-encoded SHA-256 of the fetched file (None on failure).
    """

    dep: DetectedDep
    ok: bool
    error: str | None = None
    sha256: str | None = None


# ---------------------------------------------------------------------------
# Staging directory
# ---------------------------------------------------------------------------


def _stage_root() -> Path:
    """Return ``~/.cache/xbin/stage/``, creating it if needed."""
    base = os.environ.get("XDG_CACHE_HOME")
    root = (Path(base) if base else Path.home() / ".cache") / "xbin" / "stage"
    root.mkdir(parents=True, exist_ok=True)
    return root


def _stage_key(deps: list[DetectedDep]) -> str:
    """Derive a cache key from the sorted dep list.

    Same deps → same key → cache hit on subsequent builds.
    """
    parts = sorted(f"{d.kind}:{d.name}:{d.version}" for d in deps)
    return hashlib.sha256("\n".join(parts).encode()).hexdigest()


def stage_dir_for(deps: list[DetectedDep]) -> Path:
    """Return the staging directory for a given dep list, creating subdirs."""
    root = _stage_root() / _stage_key(deps)
    for sub in ("pip", "npm", "apt", "apk", "external"):
        (root / sub).mkdir(parents=True, exist_ok=True)
    return root


# ---------------------------------------------------------------------------
# Manifest (audit log)
# ---------------------------------------------------------------------------


def _write_manifest(stage: Path, results: list[FetchResult]) -> None:
    """Write ``manifest.json`` recording all fetch outcomes."""
    entries: list[dict] = []
    for r in results:
        entry: dict = {
            "kind": r.dep.kind,
            "name": r.dep.name,
            "version": r.dep.version,
            "url": r.dep.url,
            "source": r.dep.source,
            "confidence": r.dep.confidence,
            "ok": r.ok,
        }
        if r.error:
            entry["error"] = r.error
        if r.sha256:
            entry["sha256"] = r.sha256
        entries.append(entry)
    (stage / "manifest.json").write_text(
        json.dumps({"fetched": entries}, indent=2) + "\n"
    )


# ---------------------------------------------------------------------------
# Orchestrator
# ---------------------------------------------------------------------------

# Fetcher dispatch table.
_FETCHERS: dict[str, object] = {}


def _register(kind: str, fn: object) -> None:
    _FETCHERS[kind] = fn


def fetch_deps(
    deps: list[DetectedDep],
    verbose: bool = True,
) -> tuple[Path, list[FetchResult]]:
    """Fetch all high-confidence deps into the staging directory.

    Returns ``(stage_dir, results)``.  The stage directory contains
    subdirectories per kind plus a ``manifest.json`` audit log.

    Uncertain-confidence deps are skipped with a warning.
    """
    high = [d for d in deps if d.confidence == "high"]
    uncertain = [d for d in deps if d.confidence != "high"]

    if uncertain and verbose:
        for d in uncertain:
            print(f"  SKIP (uncertain): {d.name}", file=sys.stderr)

    stage = stage_dir_for(high)
    results: list[FetchResult] = []

    for dep in high:
        fetcher = _FETCHERS.get(dep.kind)
        if fetcher is None:
            results.append(
                FetchResult(dep=dep, ok=False, error=f"unknown kind: {dep.kind}")
            )
            continue
        if verbose:
            print(f"  fetching {dep.kind} {dep.name}...", end=" ", flush=True, file=sys.stderr)
        result = fetcher(dep, stage)  # type: ignore[operator]
        results.append(result)
        if verbose:
            if result.ok:
                tag = result.sha256[:12] if result.sha256 else "ok"
                print(f"ok ({tag})", file=sys.stderr)
            else:
                print(f"WARN: {result.error}", file=sys.stderr)

    _write_manifest(stage, results)

    ok_count = sum(1 for r in results if r.ok)
    if verbose:
        print(f"[xbin] fetched {ok_count}/{len(high)} dependencies", file=sys.stderr)

    return stage, results


# ---------------------------------------------------------------------------
# pip fetcher
# ---------------------------------------------------------------------------


def _fetch_pip(dep: DetectedDep, stage: Path) -> FetchResult:
    """Download a pip package into staging via ``pip download``."""
    pip = shutil.which("pip3") or shutil.which("pip")
    if not pip:
        return FetchResult(dep=dep, ok=False, error="pip not found on PATH")

    dest = stage / "pip"
    spec = f"{dep.name}=={dep.version}" if dep.version else dep.name

    try:
        result = subprocess.run(
            [pip, "download", "--no-deps", "--dest", str(dest), spec],
            capture_output=True,
            timeout=120,
        )
        if result.returncode != 0:
            stderr = result.stderr.decode(errors="replace").strip()
            return FetchResult(dep=dep, ok=False, error=stderr[:200])

        # Find the downloaded file — wheel/sdist filenames may use
        # different casing (e.g. MarkupSafe vs markupsafe).
        downloaded = list(dest.glob("*"))
        fetched = [p for p in downloaded if p.is_file()]
        if not fetched:
            return FetchResult(dep=dep, ok=False, error="downloaded file not found")

        sha = _hash_file(fetched[0])
        return FetchResult(dep=dep, ok=True, sha256=sha)
    except subprocess.TimeoutExpired:
        return FetchResult(dep=dep, ok=False, error="pip download timed out")
    except OSError as e:
        return FetchResult(dep=dep, ok=False, error=str(e))


_register("pip", _fetch_pip)


# ---------------------------------------------------------------------------
# npm fetcher
# ---------------------------------------------------------------------------


def _fetch_npm(dep: DetectedDep, stage: Path) -> FetchResult:
    """Install an npm package into staging via ``npm install --prefix``."""
    npm = shutil.which("npm")
    if not npm:
        return FetchResult(dep=dep, ok=False, error="npm not found on PATH")

    prefix = stage / "npm"
    spec = f"{dep.name}@{dep.version}" if dep.version else dep.name

    try:
        result = subprocess.run(
            [npm, "install", "--prefix", str(prefix), "--save=false", spec],
            capture_output=True,
            timeout=120,
        )
        if result.returncode != 0:
            stderr = result.stderr.decode(errors="replace").strip()
            return FetchResult(dep=dep, ok=False, error=stderr[:200])

        # Compute hash of the installed module directory.
        mod_dir = prefix / "node_modules" / dep.name
        if mod_dir.is_dir():
            sha = _hash_dir(mod_dir)
            return FetchResult(dep=dep, ok=True, sha256=sha)
        return FetchResult(dep=dep, ok=False, error="installed module not found")
    except subprocess.TimeoutExpired:
        return FetchResult(dep=dep, ok=False, error="npm install timed out")
    except OSError as e:
        return FetchResult(dep=dep, ok=False, error=str(e))


_register("npm", _fetch_npm)


# ---------------------------------------------------------------------------
# apt fetcher
# ---------------------------------------------------------------------------


def _fetch_apt(dep: DetectedDep, stage: Path) -> FetchResult:
    """Download a .deb and extract it into staging (no ``apt-get install``)."""
    apt_get = shutil.which("apt-get")
    dpkg_deb = shutil.which("dpkg-deb")
    if not apt_get or not dpkg_deb:
        return FetchResult(
            dep=dep, ok=False, error="apt-get or dpkg-deb not found on PATH"
        )

    fetch_dir = stage / "apt" / "_fetch"
    fetch_dir.mkdir(parents=True, exist_ok=True)

    spec = f"{dep.name}={dep.version}" if dep.version else dep.name

    try:
        # apt-get download writes .deb into fetch_dir.
        result = subprocess.run(
            [apt_get, "download", spec],
            cwd=str(fetch_dir),
            capture_output=True,
            timeout=120,
        )
        if result.returncode != 0:
            stderr = result.stderr.decode(errors="replace").strip()
            return FetchResult(dep=dep, ok=False, error=stderr[:200])

        debs = list(fetch_dir.glob("*.deb"))
        if not debs:
            return FetchResult(dep=dep, ok=False, error=".deb not downloaded")

        # Extract into staging.
        pkg_dir = stage / "apt" / dep.name
        pkg_dir.mkdir(parents=True, exist_ok=True)
        for deb in debs:
            subprocess.run(
                [dpkg_deb, "-x", str(deb), str(pkg_dir)],
                check=True,
                capture_output=True,
            )

        sha = _hash_dir(pkg_dir)
        return FetchResult(dep=dep, ok=True, sha256=sha)
    except subprocess.TimeoutExpired:
        return FetchResult(dep=dep, ok=False, error="apt-get download timed out")
    except OSError as e:
        return FetchResult(dep=dep, ok=False, error=str(e))


_register("apt", _fetch_apt)


# ---------------------------------------------------------------------------
# apk fetcher
# ---------------------------------------------------------------------------


def _fetch_apk(dep: DetectedDep, stage: Path) -> FetchResult:
    """Fetch an Alpine package and extract it into staging."""
    apk = shutil.which("apk")
    if not apk:
        return FetchResult(dep=dep, ok=False, error="apk not found on PATH")

    spec = f"{dep.name}={dep.version}" if dep.version else dep.name

    try:
        # Get the download URL via --simulate.
        result = subprocess.run(
            [apk, "fetch", "--simulate", "--stdout", spec],
            capture_output=True,
            timeout=60,
        )
        if result.returncode != 0:
            stderr = result.stderr.decode(errors="replace").strip()
            return FetchResult(dep=dep, ok=False, error=stderr[:200])

        apk_bytes = result.stdout
        if not apk_bytes:
            return FetchResult(dep=dep, ok=False, error="empty response from apk fetch")

        # Write and extract.
        pkg_dir = stage / "apk" / dep.name
        pkg_dir.mkdir(parents=True, exist_ok=True)
        apk_file = pkg_dir / f"{dep.name}.apk"
        apk_file.write_bytes(apk_bytes)

        # .apk files are gzipped tarballs.
        try:
            with tarfile.open(fileobj=apk_file.open("rb"), mode="r:gz") as tf:
                tf.extractall(path=str(pkg_dir))
        except tarfile.TarError:
            pass  # not a tar — keep the raw .apk file

        sha = _hash_file(apk_file)
        return FetchResult(dep=dep, ok=True, sha256=sha)
    except subprocess.TimeoutExpired:
        return FetchResult(dep=dep, ok=False, error="apk fetch timed out")
    except OSError as e:
        return FetchResult(dep=dep, ok=False, error=str(e))


_register("apk", _fetch_apk)


# ---------------------------------------------------------------------------
# external binary fetcher
# ---------------------------------------------------------------------------


def _fetch_external(dep: DetectedDep, stage: Path) -> FetchResult:
    """Replay a wget/curl download + extract into staging."""
    if not dep.url:
        return FetchResult(dep=dep, ok=False, error="no URL provided")

    dest_dir = stage / "external" / dep.name
    dest_dir.mkdir(parents=True, exist_ok=True)

    # Determine filename from URL.
    url_name = dep.url.rstrip("/").rsplit("/", 1)[-1]
    download_path = dest_dir / url_name

    try:
        # Download with urllib (no subprocess — we want hash of the raw file).
        urllib.request.urlretrieve(dep.url, str(download_path))
    except (urllib.error.URLError, OSError) as e:
        return FetchResult(dep=dep, ok=False, error=f"download failed: {e}")

    sha = _hash_file(download_path)

    # Try to extract if it looks like an archive.
    if _is_archive(download_path):
        with contextlib.suppress(tarfile.TarError, OSError):
            _extract_archive(download_path, dest_dir)

    return FetchResult(dep=dep, ok=True, sha256=sha)


_register("external", _fetch_external)


def _is_archive(path: Path) -> bool:
    """Check if a file looks like an extractable archive."""
    lower = path.name.lower()
    return any(
        lower.endswith(ext)
        for ext in (".tar.gz", ".tgz", ".tar.xz", ".tar.bz2", ".zip")
    )


def _extract_archive(archive: Path, dest: Path) -> None:
    """Extract an archive into dest, handling common formats."""
    lower = archive.name.lower()
    if lower.endswith((".tar.gz", ".tgz", ".tar.xz", ".tar.bz2")):
        with tarfile.open(str(archive)) as tf:
            tf.extractall(path=str(dest))
    elif lower.endswith(".zip"):
        import zipfile

        with zipfile.ZipFile(str(archive)) as zf:
            zf.extractall(path=str(dest))


# ---------------------------------------------------------------------------
# Hashing utilities
# ---------------------------------------------------------------------------


def _hash_file(path: Path) -> str:
    """SHA-256 hex digest of a file."""
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def _hash_dir(directory: Path) -> str:
    """Deterministic SHA-256 of a directory tree (sorted, content-only)."""
    h = hashlib.sha256()
    for p in sorted(directory.rglob("*")):
        if p.is_file():
            h.update(str(p.relative_to(directory)).encode())
            with open(p, "rb") as f:
                for chunk in iter(lambda: f.read(65536), b""):
                    h.update(chunk)
    return h.hexdigest()
