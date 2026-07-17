"""Detect dependencies declared in a Dockerfile.

Read-only: parses RUN instructions to extract apt/apk/pip/npm packages and
external binary fetches (wget/curl + tar/unzip + chmod +x).  Returns
structured data — does not fetch or install anything.

Does NOT replace existing requirements.txt / package.json handling in
build.py and runtime.py.  Those files are parsed independently by their
own modules.  This module discovers *additional* dependencies that live
only in a Dockerfile.
"""

from __future__ import annotations

import re
import shlex
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class DetectedDep:
    """A dependency detected in a Dockerfile.

    Attributes:
        kind: One of "pip", "npm", "apt", "apk", "external".
        name: Package name, or filename for "external" fetches.
        version: Pinned version if determinable, else None.
        url: Download URL, only for kind="external".
        source: Where this was detected (currently always "Dockerfile").
    """

    kind: str
    name: str
    version: str | None = None
    url: str | None = None
    source: str = "Dockerfile"


# ---------------------------------------------------------------------------
# Public entry point
# ---------------------------------------------------------------------------


def detect_from_dockerfile(app_dir: Path) -> list[DetectedDep]:
    """Parse the Dockerfile in *app_dir* and return detected dependencies.

    Returns an empty list when no Dockerfile exists or it cannot be parsed.
    Never raises.
    """
    dockerfile = app_dir / "Dockerfile"
    if not dockerfile.is_file():
        return []

    try:
        text = dockerfile.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return []

    deps: list[DetectedDep] = []
    for run_cmd in _extract_run_blocks(text):
        # Try the multi-step chain detector on the full RUN block first.
        ext = _match_fetch_chain(run_cmd)
        if ext is not None:
            deps.append(ext)
            continue
        # Otherwise split on && and match individual package patterns.
        for sub_cmd in _split_shell_chain(run_cmd):
            deps.extend(_parse_single_command(sub_cmd))
    return deps


# ---------------------------------------------------------------------------
# Dockerfile → individual shell commands
# ---------------------------------------------------------------------------


def _extract_run_blocks(text: str) -> list[str]:
    """Extract the full shell command from each RUN instruction.

    Returns one entry per RUN instruction, with line continuations joined
    but ``&&`` / ``;`` chains left intact so the caller can detect
    multi-step patterns (e.g. wget → tar → chmod).
    """
    # Strip comments (lines where first non-whitespace is #).
    lines = [ln for ln in text.splitlines() if not ln.lstrip().startswith("#")]

    # Join line continuations: backslash at end of line → join with next.
    joined = _join_continuations("\n".join(lines))

    # Extract the shell portion after each RUN keyword.
    commands: list[str] = []
    for line in joined.splitlines():
        stripped = line.strip()
        # Match RUN at word boundary, followed by the command.
        m = re.match(r"RUN\s+(.*)", stripped, re.DOTALL)
        if m:
            commands.append(m.group(1).strip())

    return commands


def _join_continuations(text: str) -> str:
    """Join lines where the previous line ends with a backslash."""
    lines = text.split("\n")
    out: list[str] = []
    buf = ""
    for line in lines:
        if buf:
            # continuation: strip trailing backslash from buf, join.
            buf = buf.rstrip("\\").rstrip()
            buf += " " + line.strip()
        else:
            buf = line
        if buf.rstrip().endswith("\\"):
            continue  # keep accumulating
        out.append(buf)
        buf = ""
    if buf:
        out.append(buf)
    return "\n".join(out)


def _split_shell_chain(cmd: str) -> list[str]:
    """Split a shell command on && and ; separators."""
    # Split on && or ; that are not inside quotes.
    # Simple approach: use shlex to tokenize, re-split on operators.
    parts: list[str] = []
    current: list[str] = []
    try:
        tokens = shlex.split(cmd, posix=True)
    except ValueError:
        # Malformed shell — treat entire thing as one command.
        return [cmd]

    for token in tokens:
        if token in ("&&", ";"):
            if current:
                parts.append(" ".join(current))
                current = []
        else:
            current.append(token)
    if current:
        parts.append(" ".join(current))
    return parts


# ---------------------------------------------------------------------------
# Pattern matching per dependency kind
# ---------------------------------------------------------------------------

_PIP_RE = re.compile(
    r"^pip(?:3)?\s+install\s+"
    r"(?:-[^\s]*\s+)*"  # flags like -r, -q, --quiet, etc.
    r"(.+)",  # package spec(s)
)

_NPM_GLOBAL_RE = re.compile(
    r"^npm\s+(?:install|add)\s+-g\s+(?:--save-dev\s+)?(.+)",
)

_APT_RE = re.compile(
    r"^apt(?:-get)?\s+install\s+"
    r"(?:-[^\s]*\s+)*"  # flags like -y, --no-install-recommends
    r"(.+)",
)

_APK_RE = re.compile(
    r"^apk\s+add\s+(?:--no-cache\s+)?(.+)",
)


def _parse_single_command(cmd: str) -> list[DetectedDep]:
    """Match a single shell sub-command against known package patterns."""

    stripped = cmd.strip()

    m = _APT_RE.match(stripped)
    if m:
        return _split_packages(m.group(1), kind="apt")

    m = _APK_RE.match(stripped)
    if m:
        return _split_packages(m.group(1), kind="apk")

    m = _PIP_RE.match(stripped)
    if m:
        return _split_packages(m.group(1), kind="pip")

    m = _NPM_GLOBAL_RE.match(stripped)
    if m:
        return _split_packages(m.group(1), kind="npm")

    return []


def _split_packages(spec: str, kind: str) -> list[DetectedDep]:
    """Split a whitespace-separated package list into DetectedDep instances.

    Skips flags (tokens starting with -) and version specifiers (==, >=).
    Handles package names like ``flask==2.0`` or ``requests>=2.25``.
    """
    deps: list[DetectedDep] = []
    for token in spec.split():
        if token.startswith("-"):
            continue
        name, version = _split_name_version(token)
        if name:
            deps.append(DetectedDep(kind=kind, name=name, version=version))
    return deps


def _split_name_version(spec: str) -> tuple[str, str | None]:
    """Split ``flask==2.0`` into ``("flask", "2.0")``."""
    for op in ("==", ">=", "<=", "!=", "~=", ">", "<"):
        if op in spec:
            parts = spec.split(op, 1)
            return parts[0].strip(), parts[1].strip()
    return spec.strip(), None


# ---------------------------------------------------------------------------
# External binary fetch detection (wget/curl → tar/unzip → chmod +x)
# ---------------------------------------------------------------------------


def _match_fetch_chain(cmd: str) -> DetectedDep | None:
    """Match a wget/curl → tar/unzip → chmod +x chain (possibly with &&)."""
    # Normalize: split on && to get ordered steps.
    try:
        tokens = shlex.split(cmd, posix=True)
    except ValueError:
        return None

    steps = _group_chain_steps(tokens)
    if len(steps) < 2:
        return None

    url = _extract_fetch_url(steps)
    if url is None:
        return None

    has_extract = any(_is_extract_step(s) for s in steps)
    has_chmod = any(_is_chmod_step(s) for s in steps)

    if not (has_extract and has_chmod):
        return None

    name = _name_from_url(url)
    version = _version_from_url(url)
    return DetectedDep(kind="external", name=name, version=version, url=url)


def _group_chain_steps(tokens: list[str]) -> list[list[str]]:
    """Group tokens into sub-commands split by && or ;."""
    steps: list[list[str]] = []
    current: list[str] = []
    for token in tokens:
        if token in ("&&", ";"):
            if current:
                steps.append(current)
                current = []
        else:
            current.append(token)
    if current:
        steps.append(current)
    return steps


def _extract_fetch_url(steps: list[list[str]]) -> str | None:
    """Find a download URL in wget/curl sub-commands."""
    for step in steps:
        if not step:
            continue
        prog = step[0]
        if prog in ("wget", "curl"):
            # wget: URL is usually the last non-flag argument.
            # curl: URL is last argument when using -O/-o/-L.
            url = _find_url_arg(step)
            if url and _is_download_url(url):
                return url
    return None


def _find_url_arg(args: list[str]) -> str | None:
    """Extract the URL argument from wget/curl argument list."""
    last_non_flag = None
    for arg in args[1:]:
        if arg.startswith("-"):
            continue
        last_non_flag = arg
    return last_non_flag


def _is_download_url(url: str) -> bool:
    """Heuristic: does this URL look like a downloadable archive?"""
    lower = url.lower()
    return (
        any(
            lower.endswith(ext)
            for ext in (".tar.gz", ".tgz", ".tar.xz", ".tar.bz2", ".zip", ".deb")
        )
        or "github.com" in lower
    )


def _is_extract_step(tokens: list[str]) -> bool:
    """Does this sub-command extract an archive?"""
    if not tokens:
        return False
    prog = tokens[0]
    return prog in ("tar", "unzip", "ar")


def _is_chmod_step(tokens: list[str]) -> bool:
    """Does this sub-command run chmod +x?"""
    if len(tokens) < 2:
        return False
    return tokens[0] == "chmod" and "+x" in tokens[1:]


def _name_from_url(url: str) -> str:
    """Derive a human-readable name from a download URL.

    Strips common archive suffixes and version tags.
    """
    # Take the last path segment.
    name = url.rstrip("/").rsplit("/", 1)[-1]
    # Strip archive extensions.
    for ext in (".tar.gz", ".tgz", ".tar.xz", ".tar.bz2", ".zip", ".deb"):
        if name.lower().endswith(ext):
            name = name[: -len(ext)]
            break
    return name


def _version_from_url(url: str) -> str | None:
    """Try to extract a semver tag from the URL.

    Looks for patterns like ``v1.2.3``, ``1.2.3`` in the last path segment.
    """
    segment = url.rstrip("/").rsplit("/", 1)[-1]
    m = re.search(r"v?(\d+\.\d+(?:\.\d+)?)", segment)
    return m.group(1) if m else None
