"""Hugo static site generator runtime detection and embedding.

Hugo generates static sites. Detection: hugo.toml / config.toml / hugo.yaml.
Build step: hugo --minify
Serve: python3 -m http.server or a lightweight static file server.
"""

from __future__ import annotations

import shutil
from pathlib import Path

from . import Runtime, RuntimePlan


class HugoRuntime(Runtime):
    name = "hugo"

    def detect(self, app_dir: Path) -> RuntimePlan | None:
        config = _find_hugo_config(app_dir)
        if config is None:
            return None
        return _detect_hugo(app_dir)

    def supports_cross(self) -> bool:
        return True


def _find_hugo_config(app_dir: Path) -> Path | None:
    """Check for Hugo configuration files."""
    for name in ("hugo.toml", "hugo.yaml", "hugo.json", "config.toml", "config.yaml"):
        path = app_dir / name
        if path.is_file():
            # Verify it's actually a Hugo config (not generic config.toml)
            if name.startswith("hugo."):
                return path
            # For generic config.toml, check if it contains [markup] or [outputs]
            # which are Hugo-specific sections
            try:
                text = path.read_text()
                if any(
                    kw in text
                    for kw in ["baseURL", "languageCode", "[markup]", "[outputs]"]
                ):
                    return path
            except OSError:
                pass
    return None


def _detect_hugo(app_dir: Path) -> RuntimePlan:
    """Detect Hugo and configure build + serve entrypoint."""
    # Check if hugo binary is available
    hugo_bin = shutil.which("hugo")

    if hugo_bin:
        # Use Hugo to build, then serve with Python http.server
        # The entrypoint does both: hugo --minify && python3 -m http.server
        py = shutil.which("python3") or shutil.which("python") or "python3"
        py_interp = Path(py).resolve()

        return RuntimePlan(
            runtime="hugo",
            interpreter_host=Path(hugo_bin).resolve(),
            entrypoint=[
                f"/{_rootfs_rel(Path(hugo_bin).resolve())}",
                "--minify",
                "&&",
                f"/{_rootfs_rel(py_interp)}",
                "-m",
                "http.server",
                "1313",
                "--directory",
                "/app/public",
            ],
            cwd="/app",
        )

    # No hugo on PATH — we'll need to build static files during build step
    # and embed a static file server
    py = shutil.which("python3") or shutil.which("python") or "python3"
    py_interp = Path(py).resolve()

    return RuntimePlan(
        runtime="hugo",
        interpreter_host=py_interp,
        entrypoint=[
            f"/{_rootfs_rel(py_interp)}",
            "-m",
            "http.server",
            "1313",
            "--directory",
            "/app/public",
        ],
        cwd="/app",
    )


def _rootfs_rel(host_path: Path) -> str:
    return str(host_path).lstrip("/")
