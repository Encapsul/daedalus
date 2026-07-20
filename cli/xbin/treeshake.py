"""xbin tree-shaking: detect which node_modules packages are actually used.

Scans app source files for require() and import statements, resolves which
npm packages are directly imported, and returns the set of used packages.
This allows excluding unused node_modules to reduce binary size.
"""

from __future__ import annotations

import json
import re
from pathlib import Path

# Patterns that indicate a package import (not relative/absolute paths).
_REQ_RE = re.compile(r"""require\s*\(\s*['"]([^'"]+)['"]\s*\)""")
_IMPORT_RE = re.compile(
    r"""(?:import\s+(?:.*?\s+from\s+)?['"]([^'"]+)['"]"""
    r"""|import\s*\(\s*['"]([^'"]+)['"]\s*\))"""
)
# Skip these file extensions (not JS/TS).
_SKIP_EXT = {
    ".md",
    ".txt",
    ".json",
    ".yaml",
    ".yml",
    ".toml",
    ".lock",
    ".css",
    ".scss",
    ".less",
    ".svg",
    ".png",
    ".jpg",
    ".gif",
    ".woff",
    ".woff2",
    ".ttf",
    ".eot",
}
# Skip these directories when scanning.
_SKIP_DIRS = {
    "node_modules",
    ".git",
    ".venv",
    "venv",
    "dist",
    "build",
    "__pycache__",
    ".next",
    ".nuxt",
    ".output",
    "coverage",
}


def _parse_package_json(app_dir: Path) -> dict[str, str]:
    """Read package.json dependencies + devDependencies."""
    pkg = app_dir / "package.json"
    if not pkg.is_file():
        return {}
    try:
        data = json.loads(pkg.read_text())
    except (json.JSONDecodeError, OSError):
        return {}
    deps: dict[str, str] = {}
    deps.update(data.get("dependencies", {}))
    deps.update(data.get("devDependencies", {}))
    return deps


def _scan_imports_in_file(path: Path) -> set[str]:
    """Scan a single JS/TS file for require() and import specifiers."""
    try:
        content = path.read_text(errors="replace")
    except OSError:
        return set()
    found: set[str] = set()
    for m in _REQ_RE.finditer(content):
        found.add(m.group(1))
    for m in _IMPORT_RE.finditer(content):
        spec = m.group(1) or m.group(2)
        if spec:
            found.add(spec)
    return found


def _is_package_spec(spec: str) -> bool:
    """Check if an import specifier refers to a package (not relative/absolute)."""
    if spec.startswith(".") or spec.startswith("/"):
        return False
    # Scoped packages: @scope/name
    if spec.startswith("@"):
        parts = spec.split("/")
        return len(parts) >= 2
    # Regular packages: name or name/subpath
    return "/" not in spec or spec.count("/") == 0


def _extract_package_name(spec: str) -> str:
    """Extract the package name from an import specifier."""
    if spec.startswith("@"):
        parts = spec.split("/")
        if len(parts) >= 2:
            return parts[0] + "/" + parts[1]
        return spec
    return spec.split("/")[0]


def detect_used_packages(app_dir: Path) -> set[str]:
    """Scan app source files and return the set of used npm package names.

    Only scans JS/TS/JSX/TSX files. Skips node_modules, dist, etc.
    Returns the set of top-level package names that are directly imported.
    """
    pkg_deps = _parse_package_json(app_dir)
    if not pkg_deps:
        return set()

    all_imports: set[str] = set()

    js_exts = {".js", ".ts", ".jsx", ".tsx", ".mjs", ".cjs"}
    for p in app_dir.rglob("*"):
        if not p.is_file():
            continue
        if p.suffix not in js_exts:
            continue
        if any(part in _SKIP_DIRS for part in p.relative_to(app_dir).parts):
            continue
        all_imports |= _scan_imports_in_file(p)

    used: set[str] = set()
    for spec in all_imports:
        if not _is_package_spec(spec):
            continue
        pkg_name = _extract_package_name(spec)
        if pkg_name in pkg_deps:
            used.add(pkg_name)

    return used


def prune_node_modules(app_dir: Path, verbose: bool = False) -> int:
    """Remove unused packages from node_modules. Returns number removed.

    Only removes top-level directories in node_modules that are not
    directly imported by any app source file.
    """
    nm = app_dir / "node_modules"
    if not nm.is_dir():
        return 0

    used = detect_used_packages(app_dir)
    if not used:
        return 0

    removed = 0
    for child in nm.iterdir():
        if not child.is_dir():
            continue
        # Scoped packages: @scope/name
        if child.name.startswith("@"):
            for sub in child.iterdir():
                pkg_name = child.name + "/" + sub.name
                if pkg_name not in used and sub.is_dir():
                    import shutil

                    shutil.rmtree(sub)
                    removed += 1
                    if verbose:
                        print(f"  tree-shake: removed {pkg_name}", flush=True)
        else:
            if child.name not in used:
                import shutil

                shutil.rmtree(child)
                removed += 1
                if verbose:
                    print(f"  tree-shake: removed {child.name}", flush=True)

    return removed
