"""Terminal color support with NO_COLOR / --no-color / TTY detection."""

from __future__ import annotations

import os
import sys

_enabled: bool | None = None


def _should_use_color(*, no_color: bool = False) -> bool:
    if no_color:
        return False
    if os.environ.get("NO_COLOR"):
        return False
    if os.environ.get("TERM") == "dumb":
        return False
    return sys.stderr.isatty()


def init(*, no_color: bool = False) -> None:
    global _enabled
    _enabled = _should_use_color(no_color=no_color)


def is_enabled() -> bool:
    if _enabled is None:
        init()
    return _enabled  # type: ignore[return-value]


def _wrap(code: str, text: str) -> str:
    if not is_enabled():
        return text
    return f"\033[{code}m{text}\033[0m"


def red(text: str) -> str:
    return _wrap("31", text)


def green(text: str) -> str:
    return _wrap("32", text)


def yellow(text: str) -> str:
    return _wrap("33", text)


def bold(text: str) -> str:
    return _wrap("1", text)
