"""xbin doctor: check that all required and optional tools are installed."""

from __future__ import annotations

import json
import platform
import shutil
import subprocess
import sys


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


def _collect_checks() -> list[dict]:
    """Run all checks and return structured results."""
    checks: list[tuple[str, bool, str, bool]] = []

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

    return [
        {"name": name, "ok": ok, "detail": detail, "required": required}
        for name, ok, detail, required in checks
    ]


def doctor(*, verbose: bool = True, json_output: bool = False) -> int:
    """Check all prerequisites. Returns 0 if all required checks pass."""
    results = _collect_checks()

    if json_output:
        output = {
            "ok": all(r["ok"] or not r["required"] for r in results),
            "checks": results,
        }
        print(json.dumps(output, indent=2))
        return 0 if output["ok"] else 1

    required_failed = False
    for r in results:
        marker = " " if r["ok"] else ("X" if r["required"] else "-")
        if verbose:
            from ._color import green, red, yellow

            if r["ok"]:
                status = green(f"[{marker}]")
            elif r["required"]:
                status = red(f"[{marker}]")
            else:
                status = yellow(f"[{marker}]")
            req_label = "" if r["required"] else " (optional)"
            print(f"  {status:9s} {r['name']:15s} {r['detail']}{req_label}")
        if not r["ok"] and r["required"]:
            required_failed = True

    if required_failed:
        if verbose:
            from ._color import red as _red
            print(f"\n{_red('Some required tools are missing. Install them and re-run.')}")
        return 1

    if verbose:
        from ._color import green as _green
        print(f"\n{_green('All required checks passed.')}")
    return 0
