"""Shared utilities — no imports from sibling xbin modules (avoids circular deps)."""

from __future__ import annotations

import os
import platform
import sys
from pathlib import Path

_HOST_MACHINE = platform.machine()
_HOST_TARGET = f"{_HOST_MACHINE}-unknown-linux-musl"

_LEGACY_HOME = Path.home() / ".xbin"


def _xdg_data_home() -> Path:
    xdg = os.environ.get("XDG_DATA_HOME")
    if xdg:
        return Path(xdg)
    return Path.home() / ".local" / "share"


def _resolve_data_dir(subdir: str, legacy_name: str) -> Path:
    """Resolve an XDG data directory with backward-compat fallback.

    Checks `$XDG_DATA_HOME/xbin/{subdir}` first.  If it doesn't exist but
    the legacy path `~/.xbin/{legacy_name}` does, returns the legacy path
    and prints a deprecation warning.
    """
    xdg_path = _xdg_data_home() / "xbin" / subdir
    legacy_path = _LEGACY_HOME / legacy_name
    if xdg_path.exists() or not legacy_path.exists():
        return xdg_path
    # Legacy path exists, XDG doesn't — use legacy with warning.
    print(
        f"[xbin] warning: {legacy_path} is deprecated, "
        f"move to {xdg_path} (see XDG Base Directory Specification)",
        file=sys.stderr,
    )
    return legacy_path


def keys_dir() -> Path:
    return _resolve_data_dir("keys", "keys")


def trusted_dir() -> Path:
    return _resolve_data_dir("trusted-keys", "trusted-keys")


def cache_dir() -> Path:
    """Same logic as the launcher (stub/src/main.rs::cache_dir)."""
    xdg = os.environ.get("XDG_CACHE_HOME")
    if xdg:
        return Path(xdg) / "xbin"
    return Path.home() / ".cache" / "xbin"


def find_binary(
    name: str, env_var: str, error_msg: str, target_arch: str | None = None
) -> Path:
    """Locate a compiled Rust binary in standard cargo output directories.

    If target_arch is set, search for that architecture's binary instead
    of the host architecture's.  Searches repo/target, /tmp/xbin-stub-target,
    and the env_var override.

    Raises FileNotFoundError with error_msg if not found.
    """
    target_triple = f"{target_arch}-unknown-linux-musl" if target_arch else _HOST_TARGET

    here = Path(__file__).resolve()
    repo = here.parents[2]  # cli/xbin/_util.py -> repo root
    tmp_target = Path("/tmp/xbin-stub-target")
    candidates = [
        repo / f"stub/target/{target_triple}/release/{name}",
        repo / f"stub/target/release/{name}",
        tmp_target / f"{target_triple}/release/{name}",
        tmp_target / f"release/{name}",
        repo / f"stub/target/{target_triple}/debug/{name}",
        repo / f"stub/target/debug/{name}",
        tmp_target / f"{target_triple}/debug/{name}",
        tmp_target / f"debug/{name}",
    ]
    env = os.environ.get(env_var)
    if env:
        candidates.insert(0, Path(env))
    for c in candidates:
        if c.is_file():
            return c
    raise FileNotFoundError(error_msg)
