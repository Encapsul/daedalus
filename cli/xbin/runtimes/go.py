"""Go runtime detection and embedding."""

from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

from . import Runtime, RuntimePlan


class GoRuntime(Runtime):
    name = "go"

    def detect(self, app_dir: Path) -> RuntimePlan | None:
        if not (app_dir / "go.mod").is_file():
            return None
        return _detect_go(app_dir)

    def supports_cross(self) -> bool:
        return True


def _detect_go(app_dir: Path) -> RuntimePlan:
    go = shutil.which("go")
    if not go:
        raise ValueError("Go app detected (go.mod) but no go on PATH to build")

    go_bin = Path(go).resolve()

    # Build the Go binary
    build_output = app_dir / "app"
    subprocess.run(
        ["go", "build", "-o", str(build_output), "."],
        cwd=app_dir,
        check=True,
    )

    return RuntimePlan(
        runtime="go",
        interpreter_host=go_bin,
        entrypoint=["/app/app"],
        cwd="/app",
    )
