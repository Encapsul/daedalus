"""Shared utilities — no imports from sibling xbin modules (avoids circular deps)."""

from __future__ import annotations

import os
import platform
from pathlib import Path

_HOST_TARGET = f"{platform.machine()}-unknown-linux-musl"


def find_binary(name: str, env_var: str, error_msg: str) -> Path:
    """Locate a compiled Rust binary in standard cargo output directories.

    Searches repo/target, /tmp/xbin-stub-target, and the env_var override.
    Raises FileNotFoundError with error_msg if not found.
    """
    here = Path(__file__).resolve()
    repo = here.parents[2]  # cli/xbin/_util.py -> repo root
    tmp_target = Path("/tmp/xbin-stub-target")
    candidates = [
        repo / f"stub/target/{_HOST_TARGET}/release/{name}",
        repo / f"stub/target/release/{name}",
        tmp_target / f"{_HOST_TARGET}/release/{name}",
        tmp_target / f"release/{name}",
        repo / f"stub/target/{_HOST_TARGET}/debug/{name}",
        repo / f"stub/target/debug/{name}",
        tmp_target / f"{_HOST_TARGET}/debug/{name}",
        tmp_target / f"debug/{name}",
    ]
    env = os.environ.get(env_var)
    if env:
        candidates.insert(0, Path(env))
    for c in candidates:
        if c.is_file():
            return c
    raise FileNotFoundError(error_msg)
