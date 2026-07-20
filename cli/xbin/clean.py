"""xbin clean: remove local extraction cache entries."""

from __future__ import annotations

import shutil
import sys
from pathlib import Path

from ._util import cache_dir


def _dir_size(path: Path) -> int:
    total = 0
    for p in path.rglob("*"):
        if p.is_file() and not p.is_symlink():
            total += p.stat().st_size
    return total


def clean(all_entries: bool = False, force: bool = False) -> None:
    cache = cache_dir()
    if not cache.is_dir():
        print("[xbin] cache is empty", file=sys.stderr)
        return

    if all_entries:
        if not force and sys.stdin.isatty():
            size = _dir_size(cache)
            print(
                f"[xbin] this will remove the entire cache ({size/1e6:.1f}MB) at {cache}",
                file=sys.stderr,
            )
            answer = input("continue? [y/N] ").strip().lower()
            if answer not in ("y", "yes"):
                print("[xbin] aborted", file=sys.stderr)
                return
        size = _dir_size(cache)
        shutil.rmtree(cache, ignore_errors=True)
        print(
            f"[xbin] removed entire cache ({size/1e6:.1f}MB) at {cache}",
            file=sys.stderr,
        )
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
    print(
        f"[xbin] removed {removed} extracted entr{'y' if removed == 1 else 'ies'} "
        f"({freed/1e6:.1f}MB freed); build cache kept (use --all to wipe)",
        file=sys.stderr,
    )
