"""xbin selftest: launch a .xbin in an ephemeral sandbox to confirm it starts.

Two-phase detection:
  Phase 1 (0-2s):  crash check - non-zero exit or signal = exit 1.
  Phase 2 (2-T):   liveness - process still alive or exited 0 = exit 0.
Optional HTTP probe (--probe) upgrades to exit 2 if the server isn't responding.

Exit codes:
  0  started and stayed alive T seconds / exited 0
  1  crashed or failed to start
  2  alive but HTTP health check failed (--probe only)
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

from . import format as fmt


def selftest(
    file: str,
    *,
    mode: str = "auto",
    timeout: int = 3,
    probe: str | None = None,
    verbose: bool = False,
) -> int:
    path = os.path.abspath(file)
    if not os.path.isfile(path):
        print(f"[xbin] error: {path}: not found", file=sys.stderr)
        return 1

    # Read footer + metadata (no extraction needed).
    footer = fmt.read_footer(path)
    with open(path, "rb") as f:
        f.seek(footer.meta_offset)
        meta = json.loads(f.read(footer.meta_size))

    # Determine mode.
    if mode == "auto":
        mode = _detect_mode(meta)
        if verbose:
            print(f"[xbin] selftest: auto-detected mode={mode}", file=sys.stderr)

    effective_timeout = 2 + timeout
    if verbose:
        rt = meta.get("runtime", "?")
        ep = " ".join(meta.get("entrypoint", []))
        print(
            f"[xbin] selftest: {os.path.basename(path)}  runtime={rt}  "
            f"mode={mode}  timeout={effective_timeout}s",
            file=sys.stderr,
        )
        if ep:
            print(f"[xbin] selftest: entrypoint={ep}", file=sys.stderr)

    # Ephemeral cache: never touch ~/.cache/xbin/.
    with tempfile.TemporaryDirectory(prefix="xbin-selftest-") as tmp:
        env = {**os.environ, "XDG_CACHE_HOME": tmp}
        # Discard output in server mode; capture in CLI mode for diagnostics.
        kwargs: dict = {"env": env, "start_new_session": True}
        if mode == "cli":
            kwargs["stdout"] = subprocess.PIPE
            kwargs["stderr"] = subprocess.STDOUT

        try:
            proc = subprocess.Popen([path], **kwargs)
        except OSError as e:
            print(f"[xbin] error: failed to launch: {e}", file=sys.stderr)
            return 1

        if verbose:
            print(f"[xbin] selftest: started pid={proc.pid}", file=sys.stderr)

        rc = _wait_and_observe(proc, timeout, mode, probe, verbose)

    if verbose:
        label = {0: "PASS", 1: "FAIL", 2: "DEGRADED"}.get(rc, f"rc={rc}")
        print(f"[xbin] selftest: {label}", file=sys.stderr)

    return rc


def _detect_mode(meta: dict) -> str:
    """Heuristic: is this a long-running server or a short-lived CLI tool?"""
    if meta.get("services"):
        return "server"

    runtime = meta.get("runtime", "")
    if runtime in ("python", "node"):
        entry = " ".join(meta.get("entrypoint", [])).lower()
        server_hints = (
            "flask", "uvicorn", "gunicorn", "django", "http.server",
            "fastapi", "starlette", "bottle", "tornado", "aiohttp",
            "express", "hono", "fastify", "http",
        )
        if any(h in entry for h in server_hints):
            return "server"
        # Default for python/node: server (most xbin apps are web servers).
        return "server"

    return "cli"


def _wait_and_observe(
    proc: subprocess.Popen,
    timeout: int,
    mode: str,
    probe: str | None,
    verbose: bool,
) -> int:
    crash_deadline = time.monotonic() + 2.0
    end = time.monotonic() + timeout

    # Phase 1: crash check (0-2s).
    while time.monotonic() < crash_deadline:
        rc = proc.poll()
        if rc is not None:
            if rc != 0:
                _report_exit(proc, rc, mode)
                return 1
            return 0  # clean early exit (CLI tool)
        time.sleep(0.1)

    # Phase 2: liveness (2s-T).
    while time.monotonic() < end:
        rc = proc.poll()
        if rc is not None:
            return 0 if rc == 0 else 1
        time.sleep(0.1)

    # Observation window elapsed — process is alive.
    if probe:
        return _do_probe(proc, probe, verbose)
    return 0


def _report_exit(proc: subprocess.Popen, rc: int, mode: str) -> None:
    label = "crashed" if rc != 0 else "exited"
    msg = f"[xbin] selftest: {label} with code {rc}"
    if mode == "cli" and proc.stdout is not None:
        out = proc.stdout.read()
        if out:
            lines = out.decode("utf-8", errors="replace").strip().splitlines()
            tail = "\n".join(lines[-20:])
            msg += f"\n--- output (last 20 lines) ---\n{tail}"
    print(msg, file=sys.stderr)


def _do_probe(proc: subprocess.Popen, probe: str, verbose: bool) -> int:
    deadline = time.monotonic() + 3.0
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            return 1
        try:
            req = urllib.request.Request(probe, method="GET")
            with urllib.request.urlopen(req, timeout=2) as resp:
                if 200 <= resp.status < 400:
                    if verbose:
                        print(
                            f"[xbin] selftest: probe {probe} → {resp.status}",
                            file=sys.stderr,
                        )
                    return 0
        except (urllib.error.URLError, OSError, ValueError):
            pass
        time.sleep(0.3)

    print(
        f"[xbin] selftest: alive but probe {probe} failed (not responding)",
        file=sys.stderr,
    )
    return 2
