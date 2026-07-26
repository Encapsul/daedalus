"""Hugo static site generator runtime detection and embedding.

Hugo generates static sites. Detection: hugo.toml / config.toml / hugo.yaml.
Build step: hugo --minify (runs during xbin build, not at runtime).
Serve: python3 -m http.server on port 1313.
"""

from __future__ import annotations

import shutil
import subprocess
import sys
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
    py = shutil.which("python3") or shutil.which("python") or "python3"
    py_interp = Path(py).resolve()

    # Run hugo --minify during build to generate public/.
    _run_hugo_build(app_dir)

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


def _run_hugo_build(app_dir: Path) -> None:
    """Run hugo --minify to generate the static site in public/."""
    hugo_bin = shutil.which("hugo")
    if hugo_bin is None:
        print(
            "[xbin] warning: hugo not found on PATH, skipping static build "
            "(public/ must already exist)",
            file=sys.stderr,
        )
        return

    print("[xbin] running hugo --minify...", file=sys.stderr)
    result = subprocess.run(
        [hugo_bin, "--minify"],
        cwd=app_dir,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        print(f"[xbin] warning: hugo build failed:\n{result.stderr}", file=sys.stderr)
    elif result.stdout:
        # Print last few lines of hugo output
        lines = result.stdout.strip().splitlines()
        for line in lines[-5:]:
            print(f"  {line}", file=sys.stderr)


def _rootfs_rel(host_path: Path) -> str:
    return str(host_path).lstrip("/")
