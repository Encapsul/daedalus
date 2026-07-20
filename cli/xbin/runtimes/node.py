"""Node.js runtime detection and embedding."""

from __future__ import annotations

import json
import shutil
from pathlib import Path

from . import Runtime, RuntimePlan


class NodeRuntime(Runtime):
    name = "node"

    def detect(self, app_dir: Path) -> RuntimePlan | None:
        if not (app_dir / "package.json").is_file():
            return None
        return _detect_node(app_dir)


def _detect_node(app_dir: Path) -> RuntimePlan:
    node = shutil.which("node")
    if not node:
        raise ValueError(
            "node app detected (package.json) but no node on PATH to embed"
        )
    interp = Path(node).resolve()
    entry = _node_entry(app_dir)
    env: dict[str, str] = {}
    site_packages: list[tuple[Path, str]] = []
    nm = app_dir / "node_modules"
    if nm.is_dir():
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


def _rootfs_rel(host_path: Path) -> str:
    return str(host_path).lstrip("/")


def _node_entry(app_dir: Path) -> str:
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
