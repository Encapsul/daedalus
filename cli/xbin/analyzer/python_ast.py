"""Detect external binary calls in Python source via AST analysis.

Walks ``ast.Call`` nodes to find ``subprocess.run``/``Popen``/``call``/
``check_output`` and ``os.system``/``os.popen`` invocations, extracting
literal binary names from their arguments.

This module targets Python only.  Node.js subprocess detection
(``child_process.execSync``/``spawnSync``) requires a JS parser and is
not yet implemented — see HANDOFF.md for the gap.

Does NOT replace Dockerfile detection (``dockerfile.py``).  The two
modules are complementary: Dockerfile declares what to install, this
module finds what the code actually calls.
"""

from __future__ import annotations

import ast
from pathlib import Path

from .dockerfile import DetectedDep

# ---------------------------------------------------------------------------
# Public entry point
# ---------------------------------------------------------------------------


def detect_from_python_source(app_dir: Path) -> list[DetectedDep]:
    """Scan all ``.py`` files in *app_dir* for external binary calls.

    Returns an empty list when no Python files exist or none contain
    subprocess/os.system calls.  Never raises.
    """
    py_files = _find_python_files(app_dir)
    if not py_files:
        return []

    deps: list[DetectedDep] = []
    for py_file in py_files:
        deps.extend(_scan_file(py_file))
    return deps


# ---------------------------------------------------------------------------
# Merge utility
# ---------------------------------------------------------------------------


def merge_deps(
    dockerfile_deps: list[DetectedDep],
    ast_deps: list[DetectedDep],
) -> list[DetectedDep]:
    """Merge results from Dockerfile and AST scanning without duplicates.

    Deduplication key: normalized binary name (lowercased, basename only).

    When the same binary appears in both sources, the Dockerfile entry
    wins — it's an explicit, version-controlled declaration.  The AST
    entry is discarded.
    """
    seen: dict[str, DetectedDep] = {}

    for dep in dockerfile_deps:
        key = _normalize_name(dep.name)
        seen[key] = dep

    for dep in ast_deps:
        key = _normalize_name(dep.name)
        if key not in seen:
            seen[key] = dep
        # If already present from Dockerfile, skip — Dockerfile is authoritative.

    return list(seen.values())


def _normalize_name(name: str) -> str:
    """Normalize a binary/package name for dedup comparison."""
    # Strip path prefixes: "/usr/bin/ffmpeg" → "ffmpeg"
    name = name.rsplit("/", 1)[-1]
    # Strip archive suffixes that may differ between sources.
    for ext in (".tar.gz", ".tgz", ".tar.xz", ".tar.bz2", ".zip"):
        if name.lower().endswith(ext):
            name = name[: -len(ext)]
            break
    return name.lower()


# ---------------------------------------------------------------------------
# File discovery
# ---------------------------------------------------------------------------


def _find_python_files(app_dir: Path) -> list[Path]:
    """Collect all .py files, skipping common non-source directories."""
    skip_dirs = {
        ".git",
        "__pycache__",
        ".venv",
        "venv",
        "node_modules",
        ".xbin-venv",
        "site-packages",
        ".tox",
        ".mypy_cache",
    }
    py_files: list[Path] = []
    for p in sorted(app_dir.rglob("*.py")):
        # Skip files inside excluded directories.
        if any(part in skip_dirs for part in p.parts[len(app_dir.parts) :]):
            continue
        py_files.append(p)
    return py_files


# ---------------------------------------------------------------------------
# AST scanning
# ---------------------------------------------------------------------------

# Function names that invoke external processes.
_SUBPROCESS_FUNCTIONS = frozenset(
    {
        "subprocess.run",
        "subprocess.Popen",
        "subprocess.call",
        "subprocess.check_call",
        "subprocess.check_output",
        "subprocess.getoutput",
        "subprocess.getstatusoutput",
    }
)

_OS_FUNCTIONS = frozenset(
    {
        "os.system",
        "os.popen",
    }
)

# Binary names that are Python internals — never report these.
_BUILTIN_BINARIES = frozenset(
    {
        "python",
        "python3",
        "pip",
        "pip3",
        "node",
        "npm",
        "bash",
        "sh",
        "env",
        "true",
        "false",
        "test",
    }
)


def _scan_file(py_file: Path) -> list[DetectedDep]:
    """Parse one Python file and extract binary names from subprocess calls."""
    try:
        source = py_file.read_text(encoding="utf-8", errors="replace")
        tree = ast.parse(source, filename=str(py_file))
    except (SyntaxError, OSError):
        return []

    deps: list[DetectedDep] = []
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        deps.extend(_check_call(node))
    return deps


def _check_call(call: ast.Call) -> list[DetectedDep]:
    """Inspect a single Call node for subprocess/os.system patterns."""
    func_name = _resolve_func_name(call)
    if func_name is None:
        return []

    if func_name in _SUBPROCESS_FUNCTIONS:
        return _extract_subprocess_binary(call, func_name)
    if func_name in _OS_FUNCTIONS:
        return _extract_os_binary(call, func_name)
    return []


def _resolve_func_name(call: ast.Call) -> str | None:
    """Resolve the full function name from a Call node.

    Handles: ``subprocess.run(...)``, ``os.system(...)``, but not
    aliased imports like ``from subprocess import run; run(...)`` — those
    would need import tracking which is out of scope.
    """
    func = call.func
    if isinstance(func, ast.Attribute) and isinstance(func.value, ast.Name):
        return f"{func.value.id}.{func.attr}"
    return None


# ---------------------------------------------------------------------------
# Binary extraction from subprocess calls
# ---------------------------------------------------------------------------


def _extract_subprocess_binary(call: ast.Call, func_name: str) -> list[DetectedDep]:
    """Extract binary name from a subprocess function call.

    Handles:
      - ``subprocess.run("ffmpeg -i ...")``  → string first arg
      - ``subprocess.run(["ffmpeg", ...])``  → list first arg, first element
      - ``subprocess.run(cmd)``              → variable → uncertain
    """
    if not call.args:
        return []

    first = call.args[0]

    # Case 1: string literal — binary is the first word.
    if isinstance(first, ast.Constant) and isinstance(first.value, str):
        return _dep_from_shell_string(first.value)

    # Case 2: list/tuple literal — binary is the first element.
    if isinstance(first, (ast.List, ast.Tuple)):
        return _dep_from_list_literal(first)

    # Case 3: variable or expression — uncertain.
    return [_uncertain_dep(func_name)]


def _dep_from_shell_string(cmd: str) -> list[DetectedDep]:
    """Extract binary name from a shell command string like ``"ffmpeg -i in out"``."""
    # Split on whitespace, take first token.
    parts = cmd.split()
    if not parts:
        return []
    name = parts[0]
    # Skip if it's a shell builtin or Python itself.
    base = name.rsplit("/", 1)[-1]
    if base in _BUILTIN_BINARIES or base.startswith("-"):
        return []
    return [DetectedDep(kind="external", name=base, source="python-ast")]


def _dep_from_list_literal(node: ast.List | ast.Tuple) -> list[DetectedDep]:
    """Extract binary name from a list literal like ``["ffmpeg", "-i", ...]``."""
    if not node.elts:
        return []
    first = node.elts[0]
    if isinstance(first, ast.Constant) and isinstance(first.value, str):
        base = first.value.rsplit("/", 1)[-1]
        if base in _BUILTIN_BINARIES or base.startswith("-"):
            return []
        return [DetectedDep(kind="external", name=base, source="python-ast")]
    # First element is not a string literal — uncertain.
    return [_uncertain_dep("subprocess")]


def _uncertain_dep(context: str) -> DetectedDep:
    """Create an uncertain-confidence dep for unresolvable binary names."""
    return DetectedDep(
        kind="external",
        name=f"<uncertain: {context}>",
        source="python-ast",
        confidence="uncertain",
    )


# ---------------------------------------------------------------------------
# Binary extraction from os.system / os.popen
# ---------------------------------------------------------------------------


def _extract_os_binary(call: ast.Call, func_name: str) -> list[DetectedDep]:
    """Extract binary name from ``os.system("cmd")`` or ``os.popen("cmd")``."""
    if not call.args:
        return []

    first = call.args[0]
    if isinstance(first, ast.Constant) and isinstance(first.value, str):
        return _dep_from_shell_string(first.value)

    return [_uncertain_dep(func_name)]
