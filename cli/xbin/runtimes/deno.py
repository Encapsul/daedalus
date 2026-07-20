"""Deno runtime detection and embedding."""

from __future__ import annotations

import json
import shutil
from pathlib import Path

from . import Runtime, RuntimePlan


class DenoRuntime(Runtime):
    name = "deno"

    def detect(self, app_dir: Path) -> RuntimePlan | None:
        cfg = _first_existing(app_dir, ["deno.json", "deno.jsonc"])
        if not cfg:
            return None
        return _detect_deno(app_dir, cfg)


def _first_existing(app_dir: Path, names: list[str]) -> str | None:
    for n in names:
        if (app_dir / n).is_file():
            return n
    return None


def _detect_deno(app_dir: Path, cfg_name: str) -> RuntimePlan:
    deno = shutil.which("deno")
    if not deno:
        from ..cross import download_vendored_deno

        try:
            vendored = download_vendored_deno()
        except (FileNotFoundError, RuntimeError) as e:
            raise ValueError(
                f"deno app detected ({cfg_name}) but no deno on PATH "
                f"and vendored download failed: {e}"
            ) from e
        deno = str(vendored)
    interp = Path(deno).resolve()
    entry = _deno_entry(app_dir, cfg_name)
    return RuntimePlan(
        runtime="deno",
        interpreter_host=interp,
        entrypoint=[f"/{_rootfs_rel(interp)}", "run", "--allow-all", f"/app/{entry}"],
        cwd="/app",
    )


def _rootfs_rel(host_path: Path) -> str:
    return str(host_path).lstrip("/")


def _deno_entry(app_dir: Path, cfg_name: str) -> str:
    try:
        raw = (app_dir / cfg_name).read_text()
        if cfg_name.endswith(".jsonc"):
            lines = [ln for ln in raw.splitlines() if not ln.strip().startswith("//")]
            raw = "\n".join(lines)
        cfg = json.loads(raw)
        tasks = cfg.get("tasks", {})
        for key in ("start", "dev", "default"):
            cmd = tasks.get(key, {}).get("command") if isinstance(tasks.get(key), dict) else tasks.get(key)
            if cmd and isinstance(cmd, str):
                for part in cmd.split():
                    if part.endswith(".ts") and (app_dir / part).is_file():
                        return part
    except (ValueError, OSError):
        pass
    for cand in ("main.ts", "mod.ts", "server.ts", "app.ts", "index.ts"):
        if (app_dir / cand).is_file():
            return cand
    return "main.ts"
