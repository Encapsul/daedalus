"""Application runtime detection + entrypoint resolution.

MVP: python, node (detection only), native ELF binary. Best-effort, overridable
via a manifest (xbin.toml) — see build.py.
"""

from __future__ import annotations

import shutil
import sys
from dataclasses import dataclass, field
from pathlib import Path


@dataclass
class RuntimePlan:
    """Resolved execution plan for an app.

    All `*_host` paths are absolute paths on the build machine.
    Entrypoint paths are relative to the rootfs (start with '/').
    """

    runtime: str  # "python" | "node" | "binary"
    interpreter_host: Path | None  # runtime binary to embed (None for native)
    entrypoint: list[str]  # argv relatif au rootfs
    cwd: str = "/app"   # the launcher chdir's here before exec; "/app" is the
                         # app directory inside the rootfs
    env: dict[str, str] = field(default_factory=dict)
    extra_dirs_host: list[Path] = field(default_factory=list)  # ex: stdlib python
    # Third-party site-packages to embed: (host_source, path_in_rootfs).
    # The rootfs path is added to PYTHONPATH by the launcher via ${ROOTFS}.
    site_packages: list[tuple[Path, str]] = field(default_factory=list)


def _first_existing(app_dir: Path, names: list[str]) -> str | None:
    for n in names:
        if (app_dir / n).is_file():
            return n
    return None


def detect(app_dir: Path) -> RuntimePlan:
    """Detect the runtime. Raises ValueError if nothing is recognizable."""
    app_dir = app_dir.resolve()

    # 1. Python app: presence of a .py entry point
    py_entry = _first_existing(app_dir, ["app.py", "main.py", "__main__.py", "server.py"])
    if py_entry:
        py = shutil.which("python3") or shutil.which("python")
        if not py:
            py = sys.executable  # fallback: use self (self-hosting)
        interp = Path(py).resolve()
        stdlib = _python_stdlib(interp)

        env = {"PYTHONUNBUFFERED": "1", "PYTHONDONTWRITEBYTECODE": "1"}
        site_packages: list[tuple[Path, str]] = []
        sp_host = _find_site_packages(app_dir)
        if sp_host:
            # On embarque les site-packages sous /app/site-packages dans le rootfs
            # and add it to PYTHONPATH (resolved at runtime via ${ROOTFS}).
            site_packages.append((sp_host, "/app/site-packages"))
            env["PYTHONPATH"] = "${ROOTFS}/app/site-packages"

        return RuntimePlan(
            runtime="python",
            interpreter_host=interp,
            entrypoint=[f"/{_rootfs_rel(interp)}", f"/app/{py_entry}"],
            cwd="/app",
            env=env,
            extra_dirs_host=[stdlib] if stdlib else [],
            site_packages=site_packages,
        )

    # 2. Node app: package.json
    if (app_dir / "package.json").is_file():
        node = shutil.which("node")
        if not node:
            raise ValueError("node app detected (package.json) but no node on PATH to embed")
        interp = Path(node).resolve()
        entry = _node_entry(app_dir)
        env = {}
        site_packages: list[tuple[Path, str]] = []
        nm = _find_node_modules(app_dir)
        if nm:
            site_packages.append((nm, "/app/node_modules"))
            env["NODE_PATH"] = "${ROOTFS}/app/node_modules"
        return RuntimePlan(
            runtime="node",
            interpreter_host=interp,
            entrypoint=[f"/{_rootfs_rel(interp)}", f"/app/{entry}"],
            cwd="/app",
            env=env,
            site_packages=site_packages,
        )

    # 3. Native binary: single ELF executable
    elf = _single_elf(app_dir)
    if elf:
        return RuntimePlan(
            runtime="binary",
            interpreter_host=None,
            entrypoint=[f"/app/{elf.name}"],
            cwd="/app",
        )

    raise ValueError(
        "could not detect runtime: no app.py/main.py, no package.json, "
        "no single ELF binary. Use a manifest (xbin.toml) to declare entrypoint."
    )


def _rootfs_rel(host_path: Path) -> str:
    """Path of `host_path` once copied into the rootfs (tree is preserved)."""
    return str(host_path).lstrip("/")


def _find_site_packages(app_dir: Path) -> Path | None:
    """Locate third-party Python dependencies to embed.

    Looks, in order:
      1. a `.venv/` or `venv/` virtualenv → its lib/pythonX.Y/site-packages
      2. a vendored `site-packages/` directory at the app root

    Returns None if the app uses stdlib only.
    """
    for venv_name in (".venv", "venv"):
        venv = app_dir / venv_name
        lib = venv / "lib"
        if lib.is_dir():
            # lib/pythonX.Y/site-packages (take the first pythonX.Y found)
            for pyd in sorted(lib.glob("python*")):
                sp = pyd / "site-packages"
                if sp.is_dir():
                    return sp
    vendored = app_dir / "site-packages"
    if vendored.is_dir():
        return vendored
    return None


def _python_stdlib(interp: Path) -> Path | None:
    """Locate the Python stdlib (e.g. /usr/lib/python3.12) to embed."""
    candidate = Path(sys.base_prefix) / "lib" / f"python{sys.version_info.major}.{sys.version_info.minor}"
    if candidate.is_dir():
        return candidate
    return None


def _find_node_modules(app_dir: Path) -> Path | None:
    nm = app_dir / "node_modules"
    if nm.is_dir():
        return nm
    return None


def _node_entry(app_dir: Path) -> str:
    import json

    try:
        pkg = json.loads((app_dir / "package.json").read_text())
        main = pkg.get("main")
        if main and (app_dir / main).is_file():
            return main
    except (ValueError, OSError):
        pass
    for cand in ("index.js", "server.js", "app.js"):
        if (app_dir / cand).is_file():
            return cand
    return "index.js"


def _single_elf(app_dir: Path) -> Path | None:
    """Detect a single native ELF binary in the app directory."""
    elves = []
    for p in app_dir.iterdir():
        if p.is_file() and p.stat().st_mode & 0o111:
            try:
                with open(p, "rb") as f:
                    if f.read(4) == b"\x7fELF":
                        elves.append(p)
            except OSError:
                pass
    return elves[0] if len(elves) == 1 else None
