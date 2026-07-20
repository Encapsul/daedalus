"""xbin minification: shrink JS/TS/CSS files before packaging.

Uses terser (if installed) for JS/TS, and a simple whitespace stripper for CSS.
Falls back gracefully if terser is not available.
"""

from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

# File extensions to attempt minification on.
_JS_EXTS = {".js", ".mjs", ".cjs", ".ts", ".jsx", ".tsx"}
_CSS_EXTS = {".css"}
_SKIP_DIRS = {
    "node_modules", ".git", ".venv", "venv", "dist", "build",
    "__pycache__", ".next", ".nuxt", ".output", "coverage",
}


def _has_terser() -> bool:
    return shutil.which("terser") is not None


def _minify_js_file(path: Path) -> bool:
    """Minify a single JS/TS file using terser. Returns True on success."""
    if not _has_terser():
        return False
    try:
        result = subprocess.run(
            ["terser", str(path), "--compress", "--mangle", "-o", str(path)],
            capture_output=True,
            timeout=30,
        )
        return result.returncode == 0
    except (subprocess.TimeoutExpired, OSError):
        return False


def _minify_css_simple(content: str) -> str:
    """Simple CSS minification: strip comments, collapse whitespace."""
    import re
    # Remove comments
    content = re.sub(r"/\*.*?\*/", "", content, flags=re.DOTALL)
    # Collapse whitespace
    content = re.sub(r"\s+", " ", content)
    # Remove spaces around selectors/properties
    content = re.sub(r"\s*{\s*", "{", content)
    content = re.sub(r"\s*}\s*", "}", content)
    content = re.sub(r"\s*:\s*", ":", content)
    content = re.sub(r"\s*;\s*", ";", content)
    return content.strip()


def minify_app_dir(app_dir: Path, verbose: bool = False) -> int:
    """Minify JS/TS/CSS files in app_dir. Returns number of files minified."""
    minified = 0

    for p in app_dir.rglob("*"):
        if not p.is_file():
            continue
        if any(part in _SKIP_DIRS for part in p.relative_to(app_dir).parts):
            continue

        if p.suffix in _JS_EXTS:
            if _minify_js_file(p):
                minified += 1
                if verbose:
                    print(f"  minify: {p.name} (JS/TS)", flush=True)
        elif p.suffix in _CSS_EXTS:
            try:
                original = p.read_text(errors="replace")
                minified_content = _minify_css_simple(original)
                if len(minified_content) < len(original):
                    p.write_text(minified_content)
                    minified += 1
                    if verbose:
                        print(f"  minify: {p.name} (CSS)", flush=True)
            except OSError:
                pass

    return minified
