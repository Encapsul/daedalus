"""Perl runtime detection and embedding."""

from __future__ import annotations

import shutil
from pathlib import Path

from . import Runtime, RuntimePlan


class PerlRuntime(Runtime):
    name = "perl"

    def detect(self, app_dir: Path) -> RuntimePlan | None:
        if not (app_dir / "Makefile.PL").is_file() and not (app_dir / "cpanfile").is_file():
            return None
        return _detect_perl(app_dir)

    def supports_cross(self) -> bool:
        return False


def _detect_perl(app_dir: Path) -> RuntimePlan:
    perl = shutil.which("perl")
    if not perl:
        raise ValueError(
            "Perl app detected but no perl on PATH to embed"
        )
    interp = Path(perl).resolve()

    entry = _perl_entry(app_dir)

    env: dict[str, str] = {}

    return RuntimePlan(
        runtime="perl",
        interpreter_host=interp,
        entrypoint=[f"/{_rootfs_rel(interp)}", f"/app/{entry}"],
        cwd="/app",
        env=env,
    )


def _rootfs_rel(host_path: Path) -> str:
    return str(host_path).lstrip("/")


def _perl_entry(app_dir: Path) -> str:
    if (app_dir / "app.pl").is_file():
        return "app.pl"
    if (app_dir / "bin" / "app").is_file():
        return "bin/app"
    for cand in ("main.pl", "server.pl", "app.psgi"):
        if (app_dir / cand).is_file():
            return cand
    return "main.pl"
