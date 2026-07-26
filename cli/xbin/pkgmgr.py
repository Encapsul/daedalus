"""Package manager detection and installation.

Detects which package manager an app uses (uv, poetry, pipenv, pip for Python;
pnpm, yarn, bun, npm for Node; composer for PHP) and runs the appropriate
install command.  Priority is speed-based: uv > poetry > pipenv > pip;
pnpm > yarn > bun > npm.
"""

from __future__ import annotations

import os
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

# ── PHP package managers ───────────────────────────────────────────────

_PHP_PKG_MGRS: list[PkgManager] = [
    PkgManager(
        "composer",
        "composer.lock",
        [
            "composer",
            "install",
            "--no-dev",
            "--optimize-autoloader",
            "--no-interaction",
            "--ignore-platform-reqs",
            "--no-scripts",
        ],
    ),
]


# ---------------------------------------------------------------------------
# Binary finder (checks PATH + NVM)
# ---------------------------------------------------------------------------


def _find_binary(name: str) -> Path | None:
    """Locate a binary, checking PATH first then NVM directories."""
    import shutil

    found = shutil.which(name)
    if found:
        return Path(found).resolve()

    nvm_dir = Path.home() / ".nvm" / "versions" / "node"
    if nvm_dir.is_dir():
        for vdir in sorted(nvm_dir.iterdir(), reverse=True):
            candidate = vdir / "bin" / name
            if candidate.is_file():
                return candidate.resolve()

    return None


def _ensure_composer(verbose: bool = True) -> Path | None:
    """Ensure composer is available, downloading it if necessary."""
    composer = _find_binary("composer")
    if composer:
        return composer

    cache = Path.home() / ".cache" / "xbin" / "runtimes" / "composer"
    cache.mkdir(parents=True, exist_ok=True)
    composer_phar = cache / "composer.phar"
    if not composer_phar.is_file():
        url = "https://getcomposer.org/download/latest-stable/composer.phar"
        try:
            import urllib.request

            if verbose:
                print(
                    f"  [xbin] downloading composer to {composer_phar}", file=sys.stderr
                )
            urllib.request.urlretrieve(url, str(composer_phar))
            composer_phar.chmod(0o755)
        except Exception as e:
            if verbose:
                print(
                    f"  [xbin] warning: failed to download composer: {e}",
                    file=sys.stderr,
                )
            return None

    return composer_phar


# ---------------------------------------------------------------------------
# Detection
# ---------------------------------------------------------------------------


def detect_python_pkgmgr(app_dir: Path) -> PkgManager | None:
    """Return the first matching Python package manager, or None."""
    for pm in _PYTHON_PKG_MGRS:
        if (app_dir / pm.lock_file).is_file():
            return pm
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
        if (app_dir / pm.lock_file).is_file() and _find_binary(pm.name):
            return pm
    # Fallback: if lock file exists but manager missing, fall back to npm
    for pm in _NODE_PKG_MGRS:
        if (app_dir / pm.lock_file).is_file():
            return PkgManager("npm", pm.lock_file, ["npm", "install"])
    # No lock file but package.json exists → npm install
    return PkgManager("npm", "package.json", ["npm", "install"])


def detect_php_pkgmgr(app_dir: Path) -> PkgManager | None:
    """Return Composer if composer.json exists, else None."""
    if not (app_dir / "composer.json").is_file():
        return None
    for pm in _PHP_PKG_MGRS:
        if (app_dir / pm.lock_file).is_file():
            return pm
    return PkgManager(
        "composer",
        "composer.json",
        [
            "composer",
            "install",
            "--no-dev",
            "--optimize-autoloader",
            "--no-interaction",
            "--ignore-platform-reqs",
            "--no-scripts",
        ],
    )


def detect_pkgmgr(app_dir: Path, runtime: str) -> PkgManager | None:
    """Detect the package manager for a given runtime."""
    try:
        import xbin_core

        rust_name = xbin_core.py_detect_pkgmgr(str(app_dir), runtime)
        if rust_name is not None:
            return _name_to_pkgmgr(app_dir, rust_name)
    except ImportError:
        pass

    if runtime == "python":
        return detect_python_pkgmgr(app_dir)
    if runtime == "node":
        return detect_node_pkgmgr(app_dir)
    if runtime == "php":
        return detect_php_pkgmgr(app_dir)
    return None


def _name_to_pkgmgr(app_dir: Path, name: str) -> PkgManager | None:
    """Construct a PkgManager from a name detected by xbin_core."""
    all_mangers = _PYTHON_PKG_MGRS + _NODE_PKG_MGRS + _PHP_PKG_MGRS
    for pm in all_mangers:
        if pm.name == name:
            return pm
    if name == "pip" and (app_dir / "requirements.txt").is_file():
        return PkgManager(
            "pip", "requirements.txt", ["pip", "install", "-r", "requirements.txt"]
        )
    if name == "npm" and (app_dir / "package.json").is_file():
        return PkgManager("npm", "package.json", ["npm", "install"])
    if name == "composer" and (app_dir / "composer.json").is_file():
        return PkgManager(
            "composer",
            "composer.json",
            [
                "composer",
                "install",
                "--no-dev",
                "--optimize-autoloader",
                "--no-interaction",
                "--ignore-platform-reqs",
                "--no-scripts",
            ],
        )
    return None


# ---------------------------------------------------------------------------
# Installation
# ---------------------------------------------------------------------------


def install_deps(
    app_dir: Path,
    pm: PkgManager,
    verbose: bool,
    *,
    work_dir: Path | None = None,
    lang: str = "en",
) -> Path | None:
    """Run the package manager's install command.

    Returns the directory containing installed packages (for embedding),
    or None if the package manager handles installation in-place.
    """
    cmd = list(pm.install_cmd)

    if pm.name == "composer":
        composer = _ensure_composer(verbose=verbose)
        if composer:
            cmd[0] = str(composer)
        else:
            cmd[0] = "composer"
    else:
        binary = _find_binary(cmd[0])
        if binary is not None:
            cmd[0] = str(binary)

    if work_dir is not None:
        cmd = _rebase_cmd(cmd, app_dir, work_dir)

    if verbose:
        print(f"  {pm.name}: {' '.join(cmd)}", file=sys.stderr)

    env = os.environ.copy()
    if pm.name in ("pnpm", "yarn", "bun", "npm"):
        nvm_bin = Path.home() / ".nvm" / "versions" / "node"
        if nvm_bin.is_dir():
            node_dirs = [
                str(d / "bin") for d in nvm_bin.iterdir() if (d / "bin").is_dir()
            ]
            if node_dirs:
                env["PATH"] = (
                    os.pathsep.join(node_dirs) + os.pathsep + env.get("PATH", "")
                )

    env["LC_ALL"] = lang
    env["LANG"] = lang
    env["LANGUAGE"] = lang

    max_retries = 3
    retry_delay = 2
    last_error = ""
    for attempt in range(max_retries):
        result = subprocess.run(
            cmd,
            cwd=str(work_dir or app_dir),
            capture_output=True,
            env=env,
        )
        if result.returncode == 0:
            return _find_install_dir(app_dir, pm)

        stderr = result.stderr.decode(errors="replace").strip()
        last_error = stderr
        retryable = any(
            keyword in stderr.lower()
            for keyword in (
                "econnreset",
                "etimedout",
                "network",
                "timeout",
                "temporary failure",
            )
        )
        if retryable and attempt < max_retries - 1:
            if verbose:
                print(
                    f"  [xbin] {pm.name} attempt {attempt + 1}/{max_retries} failed: {stderr[:100]}, retrying in {retry_delay}s...",
                    file=sys.stderr,
                )
            import time

            time.sleep(retry_delay)
            retry_delay *= 2
            continue

        raise RuntimeError(
            f"{pm.name} install failed (exit {result.returncode}): {stderr}"
        )

    raise RuntimeError(
        f"{pm.name} install failed after {max_retries} attempts: {last_error}"
    )


def export_requirements(app_dir: Path, pm: PkgManager, verbose: bool) -> Path | None:
    """Export a requirements.txt from a non-pip package manager."""
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
        return app_dir / ".venv"
    if pm.name == "pipenv":
        return app_dir / ".venv"
    if pm.name == "pip":
        return None
    if pm.name in ("pnpm", "yarn", "bun", "npm"):
        return app_dir / "node_modules"
    if pm.name == "composer":
        return app_dir / "vendor"
    return None
