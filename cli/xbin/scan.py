"""xbin scan: discover and inspect .xbin files on the system."""

from __future__ import annotations

import json
import sys
from pathlib import Path

from . import _format as fmt
from ._util import cache_dir


def _is_xbin_file(path: Path) -> bool:
    """Quick check: read last 4 bytes for footer magic 0xBEEFCAFE."""
    try:
        size = path.stat().st_size
        if size < fmt.V2_FOOTER_SIZE:
            return False
        with open(path, "rb") as f:
            f.seek(-4, 2)
            return f.read(4) == b"\xfe\xca\xef\xbe"  # LE 0xBEEFCAFE
    except (OSError, PermissionError):
        return False


def _safe_inspect(path: Path) -> dict | None:
    """Read metadata from a .xbin file. Returns None on errors."""
    try:
        footer = fmt.read_footer(str(path))
        with open(path, "rb") as f:
            meta_bytes = fmt.read_at(f, footer.meta_offset, footer.meta_size)
        meta = json.loads(meta_bytes)
        arch = fmt.ARCH_NAMES.get(footer.arch, f"0x{footer.arch:02x}")
        signed = bool(footer.flags & fmt.FLAG_SIGNED)
        encrypted = bool(footer.flags & fmt.FLAG_ENCRYPTED)
        return {
            "file": str(path),
            "name": meta.get("name", ""),
            "runtime": meta.get("runtime", ""),
            "xbin_version": meta.get("xbin_version", ""),
            "architecture": arch,
            "signed": signed,
            "encrypted": encrypted,
            "created": meta.get("created", ""),
            "format_version": footer.format_version,
            "payload_format": meta.get("payload_format", ""),
        }
    except (ValueError, KeyError, OSError, json.JSONDecodeError):
        return None


def _find_xbin_files(paths: list[str]) -> list[Path]:
    """Recursively find .xbin files in the given directories."""
    found: list[Path] = []
    for base in paths:
        p = Path(base)
        if p.is_file():
            if p.suffix == ".xbin" or _is_xbin_file(p):
                found.append(p)
            continue
        if not p.is_dir():
            continue
        for f in p.rglob("*"):
            if (
                f.is_file()
                and not f.is_symlink()
                and (f.suffix == ".xbin" or _is_xbin_file(f))
            ):
                found.append(f)
    return sorted(set(found))


def _cache_stats() -> tuple[int, int]:
    """Return (entry_count, total_bytes) for the xbin cache directory."""
    cache = cache_dir()
    if not cache.is_dir():
        return 0, 0
    count = 0
    total = 0
    for entry in cache.iterdir():
        if entry.is_dir() and entry.name != "build":
            count += 1
            total += sum(
                f.stat().st_size
                for f in entry.rglob("*")
                if f.is_file() and not f.is_symlink()
            )
        elif entry.suffix == ".lock":
            count += 1
    return count, total


def scan(
    paths: list[str],
    *,
    json_output: bool = False,
) -> int:
    """Discover and inspect .xbin files. Returns exit code (0=found, 1=none)."""
    files = _find_xbin_files(paths)

    if not files:
        if not json_output:
            print("[xbin] no .xbin files found", file=sys.stderr)
        return 1

    results = []
    for f in files:
        data = _safe_inspect(f)
        if data is not None:
            results.append(data)

    cache_entries, cache_bytes = _cache_stats()

    if json_output:
        output = {
            "files": results,
            "cache": {"entries": cache_entries, "bytes": cache_bytes},
        }
        print(json.dumps(output, indent=2))
        return 0

    # Human-readable table
    print(f"Found {len(results)} .xbin file{'s' if len(results) != 1 else ''}:\n")

    # Column headers
    print(
        f"  {'FILE':<36} {'NAME':<16} {'RUNTIME':<10} {'ARCH':<10} {'SIGNED':<8} {'CREATED'}"
    )
    for r in results:
        path_str = r["file"]
        if len(path_str) > 34:
            path_str = "..." + path_str[-31:]
        signed = "yes" if r["signed"] else "no"
        created = r["created"][:10] if r["created"] else ""
        print(
            f"  {path_str:<36} {r['name']:<16} {r['runtime']:<10} "
            f"{r['architecture']:<10} {signed:<8} {created}"
        )

    if cache_entries > 0:
        print(
            f"\nCache: {cache_entries} entr{'y' if cache_entries == 1 else 'ies'}, "
            f"{cache_bytes / 1e6:.1f}MB"
        )

    return 0
