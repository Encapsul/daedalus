"""xbin doctor: check that all required and optional tools are installed."""

from __future__ import annotations

import platform
import shutil
import subprocess
import sys
from pathlib import Path


def _check(name: str, cmd: list[str] | None = None, hint: str = "") -> tuple[bool, str]:
    """Run a check. Returns (ok, detail)."""
    if cmd is None:
        path = shutil.which(name)
        if path:
            return True, path
        return False, f"{name} not found"
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=5)
        if r.returncode == 0:
            out = r.stdout.strip().splitlines()[0] if r.stdout.strip() else "ok"
            return True, out
        return False, r.stderr.strip().splitlines()[0] if r.stderr.strip() else f"exit {r.returncode}"
    except FileNotFoundError:
        return False, f"{cmd[0]} not found"
    except subprocess.TimeoutExpired:
        return False, "timed out"


def _check_musl_target() -> tuple[bool, str]:
    try:
        r = subprocess.run(
            ["rustup", "target", "list", "--installed"],
            capture_output=True, text=True, timeout=5,
        )
        target = f"{platform.machine()}-unknown-linux-musl"
        if target in r.stdout:
            return True, target
        return False, f"{target} not installed (run: rustup target add {target})"
    except FileNotFoundError:
        return False, "rustup not found"


def _check_python_version() -> tuple[bool, str]:
    v = sys.version_info
    if v >= (3, 10):
        return True, f"{v.major}.{v.minor}.{v.micro}"
    return False, f"{v.major}.{v.minor}.{v.micro} (need >= 3.10)"


def _check_package(name: str) -> tuple[bool, str]:
    try:
        mod = __import__(name)
        ver = getattr(mod, "__version__", "?")
        return True, ver
    except ImportError:
        return False, f"not installed (pip install {name})"


def _check_stub() -> tuple[bool, str]:
    from ._util import find_binary

    try:
        p = find_binary("xbin-stub", "XBIN_STUB", "")
        return True, str(p)
    except FileNotFoundError:
        return False, "not built (run: make stub)"


def _check_crypto() -> tuple[bool, str]:
    from ._util import find_binary

    try:
        p = find_binary("xbin-crypto", "XBIN_CRYPTO", "")
        return True, str(p)
    except FileNotFoundError:
        return False, "not built (run: make stub)"


def doctor(*, verbose: bool = True) -> int:
    """Check all prerequisites. Returns 0 if all required checks pass."""
    checks: list[tuple[str, bool, str, bool]] = []  # (name, ok, detail, required)

    def add(name: str, ok: bool, detail: str, required: bool = True) -> None:
        checks.append((name, ok, detail, required))

    # --- Required ---
    add("Python", *_check_python_version())
    add("pip", *(
        (True, subprocess.run([sys.executable, "-m", "pip", "--version"],
                              capture_output=True, text=True, timeout=5).stdout.strip().split()[1]
         if subprocess.run([sys.executable, "-m", "pip", "--version"],
                           capture_output=True, timeout=5).returncode == 0
         else (False, "not available"))
    ))
    add("cargo", *_check("cargo", ["cargo", "--version"]))
    add("rustc", *_check("rustc", ["rustc", "--version"]))
    add("musl target", *_check_musl_target())
    add("C compiler", *_check("cc", ["cc", "--version"]))
    add("zstd", *_check("zstd", ["zstd", "--version"]))

    # --- Optional ---
    add("mksquashfs", *_check("mksquashfs", ["mksquashfs", "-version"]), required=False)
    add("node", *_check("node", ["node", "--version"]), required=False)
    add("deno", *_check("deno", ["deno", "--version"]), required=False)

    # --- Python packages ---
    add("cryptography", *_check_package("cryptography"), required=False)
    add("ruff", *_check_package("ruff"), required=False)
    add("black", *_check_package("black"), required=False)

    # --- Built binaries ---
    add("xbin-stub", *_check_stub())
    add("xbin-crypto", *_check_crypto())

    # --- Print results ---
    required_failed = False
    for name, ok, detail, required in checks:
        tag = "OK" if ok else ("FAIL" if required else "optional")
        marker = " " if ok else ("X" if required else "-")
        if verbose:
            status = f"[{marker}]"
            req_label = "" if required else " (optional)"
            print(f"  {status:5s} {name:15s} {detail}{req_label}")
        if not ok and required:
            required_failed = True

    if required_failed:
        if verbose:
            print("\nSome required tools are missing. Install them and re-run.")
        return 1

    if verbose:
        print("\nAll required checks passed.")
    return 0
