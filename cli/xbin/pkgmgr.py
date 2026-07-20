"""Package manager detection and installation.

Detects which package manager an app uses (uv, poetry, pipenv, pip for Python;
pnpm, yarn, bun, npm for Node) and runs the appropriate install command.
Priority is speed-based: uv > poetry > pipenv > pip; pnpm > yarn > bun > npm.
"""

from __future__ import annotations

import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path


@dataclass
class PkgManager:
    name: str
    lock_file: str
    install_cmd: list[str]
    export_cmd: list[str] | None = None


# ── Python package managers (priority order) ───────────────────────────

_PYTHON_PKG_MGRS: list[PkgManager] = [
    PkgManager("uv", "uv.lock", ["uv", "sync"]),
    PkgManager(
        "poetry",
        "poetry.lock",
        ["poetry", "install", "--no-interaction"],
        export_cmd=["poetry", "export", "-f", "requirements.txt", "--without-hashes"],
    ),
    PkgManager(
        "pipenv",
        "Pipfile.lock",
        ["pipenv", "install", "--deploy"],
        export_cmd=["pipenv", "requirements"],
    ),
]

# ── Node package managers (priority order) ─────────────────────────────

_NODE_PKG_MGRS: list[PkgManager] = [
    PkgManager("pnpm", "pnpm-lock.yaml", ["pnpm", "install", "--frozen-lockfile"]),
    PkgManager("yarn", "yarn.lock", ["yarn", "install", "--frozen-lockfile"]),
    PkgManager("bun", "bun.lockb", ["bun", "install", "--frozen-lockfile"]),
    PkgManager("npm", "package-lock.json", ["npm", "ci"]),
]


def detect_python_pkgmgr(app_dir: Path) -> PkgManager | None:
    """Return the first matching Python package manager, or None."""
    for pm in _PYTHON_PKG_MGRS:
        if (app_dir / pm.lock_file).is_file():
            return pm
    # Fallback: requirements.txt → pip
    req = app_dir / "requirements.txt"
    if req.is_file() and req.stat().st_size > 0:
        return PkgManager(
            "pip", "requirements.txt", ["pip", "install", "-r", "requirements.txt"]
        )
    return None


def detect_node_pkgmgr(app_dir: Path) -> PkgManager | None:
    """Return the first matching Node package manager, or None."""
    if not (app_dir / "package.json").is_file():
        return None
    for pm in _NODE_PKG_MGRS:
        if (app_dir / pm.lock_file).is_file():
            return pm
    # No lock file but package.json exists → npm install
    return PkgManager("npm", "package.json", ["npm", "install"])


def detect_pkgmgr(app_dir: Path, runtime: str) -> PkgManager | None:
    """Detect the package manager for a given runtime."""
    if runtime == "python":
        return detect_python_pkgmgr(app_dir)
    if runtime == "node":
        return detect_node_pkgmgr(app_dir)
    return None


def install_deps(
    app_dir: Path,
    pm: PkgManager,
    verbose: bool,
    *,
    work_dir: Path | None = None,
) -> Path | None:
    """Run the package manager's install command.

    Returns the directory containing installed packages (for embedding),
    or None if the package manager handles installation in-place.
    """
    cmd = pm.install_cmd
    if work_dir is not None:
        cmd = _rebase_cmd(cmd, app_dir, work_dir)

    if verbose:
        print(f"  {pm.name}: {' '.join(cmd)}", file=sys.stderr)

    result = subprocess.run(
        cmd,
        cwd=str(work_dir or app_dir),
        capture_output=True,
    )

    if result.returncode != 0:
        stderr = result.stderr.decode(errors="replace").strip()
        raise RuntimeError(
            f"{pm.name} install failed (exit {result.returncode}): {stderr}"
        )

    return _find_install_dir(app_dir, pm)


def export_requirements(app_dir: Path, pm: PkgManager, verbose: bool) -> Path | None:
    """Export a requirements.txt from a non-pip package manager.

    Used for cross-compilation where we need a requirements.txt for
    pip download --only-binary.
    """
    if pm.export_cmd is None:
        return None

    if verbose:
        print(f"  {pm.name}: {' '.join(pm.export_cmd)}", file=sys.stderr)

    result = subprocess.run(
        pm.export_cmd,
        cwd=str(app_dir),
        capture_output=True,
    )

    if result.returncode != 0:
        if verbose:
            stderr = result.stderr.decode(errors="replace").strip()
            print(
                f"  warning: {pm.name} export failed: {stderr}",
                file=sys.stderr,
            )
        return None

    req_path = app_dir / "requirements.txt"
    req_path.write_bytes(result.stdout)
    if verbose:
        print(f"  exported {pm.name} deps -> {req_path}", file=sys.stderr)
    return req_path


def _rebase_cmd(cmd: list[str], original: Path, work_dir: Path) -> list[str]:
    """Rebase a command's working directory references."""
    return cmd


def _find_install_dir(app_dir: Path, pm: PkgManager) -> Path | None:
    """Find where packages were installed, for embedding."""
    if pm.name == "uv":
        return app_dir / ".venv"
    if pm.name == "poetry":
        # poetry installs into .venv by default
        return app_dir / ".venv"
    if pm.name == "pipenv":
        return app_dir / ".venv"
    if pm.name == "pip":
        return None  # handled separately by layers.py
    # Node managers install into node_modules in-place
    if pm.name in ("pnpm", "yarn", "bun", "npm"):
        return app_dir / "node_modules"
    return None
