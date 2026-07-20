"""xbin persistent storage: per-app data directory across runs.

Deno-style persistent storage: compiled apps get a stable directory in
~/.local/share/xbin/{app-name}/ that persists across runs.
"""

from __future__ import annotations

import os
from pathlib import Path


def get_persist_dir(app_name: str) -> Path:
    """Return the persistent storage directory for an app.

    Follows XDG Base Directory Specification:
      ~/.local/share/xbin/{app_name}/

    Falls back to ~/.xbin/persist/{app_name}/ if XDG_DATA_HOME is not set.
    """
    base = os.environ.get("XDG_DATA_HOME")
    if base:
        return Path(base) / "xbin" / app_name
    return Path.home() / ".local" / "share" / "xbin" / app_name


def ensure_persist_dir(app_name: str) -> Path:
    """Create (if needed) and return the persistent storage directory."""
    d = get_persist_dir(app_name)
    d.mkdir(parents=True, exist_ok=True)
    return d


def get_persist_env(app_name: str) -> dict[str, str]:
    """Return env vars to inject for persistent storage.

    Sets XBIN_PERSIST_DIR to the app's persistent directory path.
    """
    d = ensure_persist_dir(app_name)
    return {"XBIN_PERSIST_DIR": str(d)}
