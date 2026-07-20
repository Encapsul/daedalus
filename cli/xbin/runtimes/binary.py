"""Native ELF binary runtime detection."""

from __future__ import annotations

from pathlib import Path

from . import Runtime, RuntimePlan


class BinaryRuntime(Runtime):
    name = "binary"

    def detect(self, app_dir: Path) -> RuntimePlan | None:
        elf_path = _single_elf(app_dir)
        if elf_path is None:
            return None
        return RuntimePlan(
            runtime="binary",
            interpreter_host=None,
            entrypoint=[f"/app/{elf_path.name}"],
            cwd="/app",
        )


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
