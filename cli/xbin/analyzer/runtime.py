"""Détection du runtime d'une application + résolution de l'entrypoint.

MVP : python, node (détection), binaire ELF natif. Best-effort, surchargeable
par un manifest (xbin.toml) — voir build.py.
"""

from __future__ import annotations

import shutil
from dataclasses import dataclass, field
from pathlib import Path


@dataclass
class RuntimePlan:
    """Plan d'exécution résolu pour une app.

    Tous les chemins `*_host` sont des chemins absolus sur la machine de build.
    Les chemins de l'entrypoint sont relatifs au rootfs (commencent par '/').
    """

    runtime: str  # "python" | "node" | "binary"
    interpreter_host: Path | None  # binaire du runtime à embarquer (None si natif)
    entrypoint: list[str]  # argv relatif au rootfs
    cwd: str = "/app"
    env: dict[str, str] = field(default_factory=dict)
    extra_dirs_host: list[Path] = field(default_factory=list)  # ex: stdlib python
    # site-packages tiers à embarquer : (source_hôte, chemin_dans_le_rootfs).
    # Le chemin rootfs est ajouté à PYTHONPATH du launcher via ${ROOTFS}.
    site_packages: list[tuple[Path, str]] = field(default_factory=list)


def _first_existing(app_dir: Path, names: list[str]) -> str | None:
    for n in names:
        if (app_dir / n).is_file():
            return n
    return None


def detect(app_dir: Path) -> RuntimePlan:
    """Détecte le runtime. Lève ValueError si rien de reconnaissable."""
    app_dir = app_dir.resolve()

    # 1. App Python : présence d'un point d'entrée .py
    py_entry = _first_existing(app_dir, ["app.py", "main.py", "__main__.py", "server.py"])
    if py_entry:
        py = shutil.which("python3") or shutil.which("python")
        if not py:
            raise ValueError("python runtime detected but no python3 on PATH to embed")
        interp = Path(py).resolve()
        stdlib = _python_stdlib(interp)

        env = {"PYTHONUNBUFFERED": "1", "PYTHONDONTWRITEBYTECODE": "1"}
        site_packages: list[tuple[Path, str]] = []
        sp_host = _find_site_packages(app_dir)
        if sp_host:
            # On embarque les site-packages sous /app/site-packages dans le rootfs
            # et on l'ajoute à PYTHONPATH (résolu à l'exécution via ${ROOTFS}).
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

    # 2. App Node : package.json
    if (app_dir / "package.json").is_file():
        node = shutil.which("node")
        if not node:
            raise ValueError("node app detected (package.json) but no node on PATH to embed")
        interp = Path(node).resolve()
        entry = _node_entry(app_dir)
        return RuntimePlan(
            runtime="node",
            interpreter_host=interp,
            entrypoint=[f"/{_rootfs_rel(interp)}", f"/app/{entry}"],
            cwd="/app",
        )

    # 3. Binaire natif : un seul exécutable ELF
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
    """Chemin de `host_path` une fois copié dans le rootfs (on préserve l'arbo)."""
    return str(host_path).lstrip("/")


def _find_site_packages(app_dir: Path) -> Path | None:
    """Localise les dépendances tierces Python à embarquer.

    Cherche, dans l'ordre :
      1. un virtualenv `.venv/` ou `venv/`  → son lib/pythonX.Y/site-packages
      2. un dossier `site-packages/` vendu à la racine de l'app

    Retourne None si l'app n'utilise que la stdlib.
    """
    for venv_name in (".venv", "venv"):
        venv = app_dir / venv_name
        lib = venv / "lib"
        if lib.is_dir():
            # lib/pythonX.Y/site-packages (on prend le premier pythonX.Y trouvé)
            for pyd in sorted(lib.glob("python*")):
                sp = pyd / "site-packages"
                if sp.is_dir():
                    return sp
    vendored = app_dir / "site-packages"
    if vendored.is_dir():
        return vendored
    return None


def _python_stdlib(interp: Path) -> Path | None:
    """Localise la stdlib python (ex: /usr/lib/python3.12) à embarquer."""
    import sys

    candidate = Path(sys.base_prefix) / "lib" / f"python{sys.version_info.major}.{sys.version_info.minor}"
    if candidate.is_dir():
        return candidate
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
