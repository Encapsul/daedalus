"""xbin doctor: check that all required and optional tools are installed."""

from __future__ import annotations

import json
import platform
import shutil
import subprocess
import sys
from pathlib import Path

# ---------------------------------------------------------------------------
# Checks
# ---------------------------------------------------------------------------


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
        return False, (
            r.stderr.strip().splitlines()[0]
            if r.stderr.strip()
            else f"exit {r.returncode}"
        )
    except FileNotFoundError:
        return False, f"{cmd[0]} not found"
    except subprocess.TimeoutExpired:
        return False, "timed out"


def _check_musl_target() -> tuple[bool, str]:
    try:
        r = subprocess.run(
            ["rustup", "target", "list", "--installed"],
            capture_output=True,
            text=True,
            timeout=5,
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
    add(
        "pip",
        *(
            (
                True,
                (
                    subprocess.run(
                        [sys.executable, "-m", "pip", "--version"],
                        capture_output=True,
                        text=True,
                        timeout=5,
                    )
                    .stdout.strip()
                    .split()[1]
                    if subprocess.run(
                        [sys.executable, "-m", "pip", "--version"],
                        capture_output=True,
                        timeout=5,
                    ).returncode
                    == 0
                    else (False, "not available")
                ),
            )
        ),
    )
    add("cargo", *_check("cargo", ["cargo", "--version"]))
    add("rustc", *_check("rustc", ["rustc", "--version"]))
    add("musl target", *_check_musl_target())
    add("C compiler", *_check("cc", ["cc", "--version"]))
    add("zstd", *_check("zstd", ["zstd", "--version"]))

    # --- Optional ---
    add(
        "mksquashfs",
        *_check("mksquashfs", ["mksquashfs", "-version"]),
        required=False,
    )
    add("node", *_check("node", ["node", "--version"]), required=False)
    add("deno", *_check("deno", ["deno", "--version"]), required=False)

    # --- Python packages ---
    add("cryptography", *_check_package("cryptography"), required=False)
    add("ruff", *_check_package("ruff"), required=False)
    add("black", *_check_package("black"), required=False)

    # --- Built binaries ---
    add("xbin-stub", *_check_stub(), required=False)
    add("xbin-crypto", *_check_crypto(), required=False)

    return [
        {"name": name, "ok": ok, "detail": detail, "required": required}
        for name, ok, detail, required in checks
    ]


# ---------------------------------------------------------------------------
# Fixers — one per fixable check
# ---------------------------------------------------------------------------


def _fix_musl_target(verbose: bool) -> tuple[bool, str]:
    target = f"{platform.machine()}-unknown-linux-musl"
    cmd = ["rustup", "target", "add", target]
    if verbose:
        print(f"    $ {' '.join(cmd)}", file=sys.stderr)
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
        if r.returncode == 0:
            return True, target
        return False, r.stderr.strip().splitlines()[0] if r.stderr else "failed"
    except FileNotFoundError:
        return False, "rustup not found"


def _fix_zstd(verbose: bool) -> tuple[bool, str]:
    cmd = ["sudo", "apt-get", "install", "-y", "zstd"]
    if verbose:
        print(f"    $ {' '.join(cmd)}", file=sys.stderr)
    try:
        subprocess.run(cmd, capture_output=True, text=True, timeout=60)
    except FileNotFoundError:
        return False, "apt-get not found (install zstd manually)"
    return _check("zstd", ["zstd", "--version"])


def _fix_mksquashfs(verbose: bool) -> tuple[bool, str]:
    cmd = ["sudo", "apt-get", "install", "-y", "squashfs-tools"]
    if verbose:
        print(f"    $ {' '.join(cmd)}", file=sys.stderr)
    try:
        subprocess.run(cmd, capture_output=True, text=True, timeout=60)
    except FileNotFoundError:
        return False, "apt-get not found (install squashfs-tools manually)"
    return _check("mksquashfs", ["mksquashfs", "-version"])


def _fix_python_package(name: str, verbose: bool) -> tuple[bool, str]:
    cmd = [sys.executable, "-m", "pip", "install", name]
    if verbose:
        print(f"    $ {' '.join(cmd)}", file=sys.stderr)
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
        if r.returncode != 0:
            detail = r.stderr.strip().splitlines()[-1] if r.stderr else "pip failed"
            return False, detail
    except FileNotFoundError:
        return False, "pip not found"
    return _check_package(name)


def _fix_stub(verbose: bool) -> tuple[bool, str]:
    repo = Path(__file__).resolve().parents[2]
    makefile = repo / "Makefile"
    if not makefile.exists():
        return False, "Makefile not found (not in xbin repo?)"
    cmd = ["make", "stub"]
    if verbose:
        print(f"    $ {' '.join(cmd)}", file=sys.stderr)
    try:
        r = subprocess.run(
            cmd, capture_output=True, text=True, timeout=300, cwd=str(repo)
        )
        if r.returncode == 0:
            return _check_stub()
        detail = r.stderr.strip().splitlines()[-1] if r.stderr else "build failed"
        return False, detail
    except FileNotFoundError:
        return False, "make not found"


FIXERS: dict[str, tuple[callable, bool]] = {
    "musl target": (_fix_musl_target, False),
    "zstd": (_fix_zstd, False),
    "mksquashfs": (_fix_mksquashfs, False),
    "cryptography": (lambda v: _fix_python_package("cryptography", v), False),
    "ruff": (lambda v: _fix_python_package("ruff", v), False),
    "black": (lambda v: _fix_python_package("black", v), False),
    "xbin-stub": (_fix_stub, False),
    "xbin-crypto": (_fix_stub, False),
}


# ---------------------------------------------------------------------------
# Display helpers
# ---------------------------------------------------------------------------


def _print_checks(results: list[dict], *, verbose: bool) -> bool:
    """Print check results. Returns True if any required check failed."""
    from ._color import green, red, yellow

    required_failed = False
    for r in results:
        marker = " " if r["ok"] else ("X" if r["required"] else "-")
        if r["ok"]:
            status = green(f"[{marker}]")
        elif r["required"]:
            status = red(f"[{marker}]")
        else:
            status = yellow(f"[{marker}]")
        req_label = "" if r["required"] else " (optional)"
        if verbose:
            print(f"  {status:9s} {r['name']:15s} {r['detail']}{req_label}")
        if not r["ok"] and r["required"]:
            required_failed = True
    return required_failed


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def doctor(
    *,
    verbose: bool = True,
    json_output: bool = False,
    fix: bool = False,
    force: bool = False,
) -> int:
    """Check all prerequisites. Returns 0 if all required checks pass."""
    results = _collect_checks()

    if json_output:
        output = {
            "ok": all(r["ok"] or not r["required"] for r in results),
            "checks": results,
        }
        print(json.dumps(output, indent=2))
        if not fix:
            return 0 if output["ok"] else 1

    required_failed = _print_checks(results, verbose=verbose)

    # No --fix: just report.
    if not fix:
        if required_failed:
            if verbose:
                from ._color import red as _red

                msg = "Some required tools are missing. Install them and re-run."
                print(f"\n{_red(msg)}")
            return 1
        if verbose:
            from ._color import green as _green

            print(f"\n{_green('All required checks passed.')}")
        return 0

    # --fix mode --------------------------------------------------------
    failed = [r for r in results if not r["ok"] and r["required"]]
    if not failed:
        if verbose:
            from ._color import green as _green

            print(f"\n{_green('All required checks passed. Nothing to fix.')}")
        return 0

    fixable = [r for r in failed if r["name"] in FIXERS]
    unfixable = [r for r in failed if r["name"] not in FIXERS]

    if unfixable and verbose:
        from ._color import yellow as _yellow

        names = ", ".join(r["name"] for r in unfixable)
        print(
            f"\n{_yellow(f'Cannot auto-fix: {names}')}",
            file=sys.stderr,
        )
        for r in unfixable:
            print(f"  [-] {r['name']:15s} {r['detail']}", file=sys.stderr)

    if not fixable:
        return 1

    # Confirm if interactive.
    if sys.stdin.isatty() and not force:
        names = ", ".join(r["name"] for r in fixable)
        print(f"\n[xbin] will attempt to fix: {names}", file=sys.stderr)
        try:
            answer = input("Proceed? [y/N] ")
        except (EOFError, KeyboardInterrupt):
            print("", file=sys.stderr)
            return 1
        if answer.lower() not in ("y", "yes"):
            return 1

    from ._color import green as _green
    from ._color import red as _red

    if verbose:
        n = len(fixable)
        print(f"\n[xbin] fixing {n} {'issue' if n == 1 else 'issues'}...\n")

    fixed = 0
    failed_count = 0
    for r in fixable:
        fixer = FIXERS[r["name"]][0]
        if verbose:
            print(f"  [>] {r['name']:15s} fixing...", file=sys.stderr, end="")
        ok, detail = fixer(verbose)
        if ok:
            if verbose:
                print(
                    f"\r  [{_green('OK')}] {r['name']:15s} {detail}",
                    file=sys.stderr,
                )
            r["ok"] = True
            fixed += 1
        else:
            if verbose:
                print(
                    f"\r  [{_red('X')}] {r['name']:15s} {detail}",
                    file=sys.stderr,
                )
            failed_count += 1

    if verbose:
        print()
        if failed_count == 0:
            print(f"{_green('All issues fixed.')}")
        else:
            print(f"{_green(f'{fixed} fixed')}  " f"{_red(f'{failed_count} failed')}")

    if json_output:
        output = {"fixed": fixed, "failed": failed_count, "checks": results}
        print(json.dumps(output, indent=2))

    return 0 if failed_count == 0 else 1
