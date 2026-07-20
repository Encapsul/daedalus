"""Parse .env files and merge into the runtime environment.

Standard format: KEY=value, # comments, export prefix, quotes optional.
Security: no variable expansion — values are literal strings.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

_SECRET_PATTERNS = re.compile(
    r"(secret|password|token|api_key|apikey|private_key|credentials)",
    re.IGNORECASE,
)

_QUOTE_RE = re.compile(r"""^(?:'([^']*)'|"([^"]*)"|(.+))$""")


def parse_dotenv(path: Path) -> dict[str, str]:
    """Parse a .env file and return a dict of KEY=value pairs.

    Supports: KEY=value, KEY="value", KEY='value', export KEY=value.
    Lines starting with # and empty lines are ignored.
    No variable expansion — values are literal strings.
    """
    env: dict[str, str] = {}
    if not path.is_file():
        return env

    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue

        # Strip export prefix.
        if line.startswith("export "):
            line = line[7:].strip()

        # Split on first = only.
        idx = line.find("=")
        if idx < 1:
            continue

        key = line[:idx].strip()
        raw = line[idx + 1 :].strip()

        # Unwrap quotes if present.
        m = _QUOTE_RE.match(raw)
        if m:
            value = (
                m.group(1)
                if m.group(1) is not None
                else (m.group(2) if m.group(2) is not None else m.group(3))
            )
        else:
            value = raw

        if key:
            env[key] = value

    return env


def detect_secret_keys(env: dict[str, str]) -> list[str]:
    """Return keys that look like secrets (password, token, api_key, etc.)."""
    return [k for k in env if _SECRET_PATTERNS.search(k)]


def load_dotenv(
    app_dir: Path,
    env_file: str | None,
    *,
    verbose: bool = False,
) -> dict[str, str]:
    """Load a .env file from the app directory.

    Returns parsed env vars. Warns about secrets if verbose.
    Returns empty dict if no file found or no env_file specified.
    """
    if env_file is None:
        return {}

    path = Path(env_file)
    if not path.is_absolute():
        path = app_dir / env_file

    if not path.is_file():
        if verbose:
            print(f"[xbin] warning: env file not found: {path}", file=sys.stderr)
        return {}

    env = parse_dotenv(path)
    if not env:
        if verbose:
            print(f"[xbin] warning: env file is empty: {path}", file=sys.stderr)
        return {}

    secrets = detect_secret_keys(env)
    if secrets and verbose:
        print(
            f"[xbin] warning: env file contains {len(secrets)} secret-like key(s): "
            f"{', '.join(secrets[:5])}{'...' if len(secrets) > 5 else ''}",
            file=sys.stderr,
        )

    if verbose:
        print(f"[xbin] loaded {len(env)} env var(s) from {path.name}", file=sys.stderr)

    return env
