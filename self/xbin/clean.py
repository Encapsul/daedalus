"""xbin clean: remove local extraction cache entries."""

from __future__ import annotations

import os
import shutil
from pathlib import Path


def cache_dir() -> Path:
    """Same logic as the launcher (stub/src/main.rs::cache_dir)."""
    xdg = os.environ.get("XDG_CACHE_HOME")
    if xdg:
        return Path(xdg) / "xbin"
    return Path.home() / ".cache" / "xbin"


def _dir_size(path: Path) -> int:
    total = 0
    for p in path.rglob("*"):
        if p.is_file() and not p.is_symlink():
            total += p.stat().st_size
    return total


def clean(all_entries: bool = False) -> None:
    cache = cache_dir()
    if not cache.is_dir():
        print("[xbin] cache is empty")
        return

    if all_entries:
        size = _dir_size(cache)
        shutil.rmtree(cache, ignore_errors=True)
        print(f"[xbin] removed entire cache ({size/1e6:.1f}MB) at {cache}")
        return

    # Without --all: remove extracted entries ({sha256}/ dirs) and orphaned
    # locks, but keep the build cache (`build/`, which speeds up rebuilds)
    # and the cache dir itself.
    removed = 0
    freed = 0
    for entry in cache.iterdir():
        if entry.is_dir():
            if entry.name == "build":
                continue  # preserve build cache (use --all to wipe)
            freed += _dir_size(entry)
            shutil.rmtree(entry, ignore_errors=True)
            removed += 1
        elif entry.suffix == ".lock":
            entry.unlink(missing_ok=True)
    print(f"[xbin] removed {removed} extracted entr{'y' if removed == 1 else 'ies'} "
          f"({freed/1e6:.1f}MB freed); build cache kept (use --all to wipe)")
