#!/usr/bin/env python3
"""Local model kit - stdlib CLI around a real on-disk GGUF.

Honesty:
  - Weights are NOT in git. Run ./fetch_weights.sh first (~241 MB).
  - Offline inference needs those bytes in models/ AND an Ollama binary
    new enough for Gemma 3 (0.40.5+). This script does not download at run.
  - Packaged .de only contains weights if they were present at build time.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent
GGUF = ROOT / "models" / "gemma-3-270m-it-Q4_K_M.gguf"
MODELFILE = ROOT / "Modelfile"
MODEL_NAME = os.environ.get("LOCAL_MODEL_NAME", "daedalus-gemma-270m")
OLLAMA_HOST = os.environ.get("OLLAMA_HOST", "http://127.0.0.1:11434")


def probe() -> int:
    print("=== LOCAL MODEL KIT: PROBE ===")
    print(f"gguf_path={GGUF}")
    if not GGUF.is_file():
        print("weights=missing")
        print("hint=run ./fetch_weights.sh (Hugging Face unsloth/gemma-3-270m-it-GGUF)")
        return 2
    size = GGUF.stat().st_size
    magic = GGUF.read_bytes()[:4]
    print(f"weights=present bytes={size} magic={magic!r}")
    print(f"modelfile={MODELFILE.is_file()}")
    print(f"ollama_host={OLLAMA_HOST}")
    print("note=weights must be present at daedalus build time to land in the .de")
    return 0


def _http_json(method: str, path: str, body: dict | None = None, timeout: float = 120.0):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(
        f"{OLLAMA_HOST}{path}",
        data=data,
        method=method,
        headers={"Content-Type": "application/json"} if data else {},
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read().decode())


def _ollama_up() -> bool:
    try:
        _http_json("GET", "/api/tags", timeout=2.0)
        return True
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError):
        return False


def _ensure_model() -> None:
    if not GGUF.is_file():
        raise SystemExit("weights missing - run ./fetch_weights.sh")
    if not MODELFILE.is_file():
        raise SystemExit("Modelfile missing")
    # Create/overwrite from local GGUF (no registry pull).
    subprocess.run(
        ["ollama", "create", MODEL_NAME, "-f", str(MODELFILE)],
        cwd=str(ROOT),
        check=True,
    )


def ask(prompt: str) -> int:
    print("=== LOCAL MODEL KIT: ASK ===")
    if not GGUF.is_file():
        print("weights=missing - refusing to pretend offline AI", file=sys.stderr)
        return 2
    if not _ollama_up():
        print(
            f"ollama not reachable at {OLLAMA_HOST} - start: ollama serve",
            file=sys.stderr,
        )
        return 2
    _ensure_model()
    t0 = time.time()
    out = _http_json(
        "POST",
        "/api/generate",
        {
            "model": MODEL_NAME,
            "prompt": prompt,
            "stream": False,
            "options": {"num_predict": 64, "temperature": 0.2},
        },
        timeout=300.0,
    )
    text = (out.get("response") or "").strip()
    print(f"model={MODEL_NAME}")
    print(f"elapsed_s={time.time() - t0:.2f}")
    print("---")
    print(text if text else "(empty response)")
    return 0 if text else 1


def domains() -> int:
    print("=== LOCAL MODEL KIT: DOMAINS ===")
    for line in (
        "local-ai: real GGUF in models/ at build time → bytes inside the .de",
        "airgap-courier: ship kit.de + .pub; recipient trust then verify",
        "not-a-claim: without models/*.gguf the artifact is NOT an offline LLM",
        "host-dep: needs Ollama >=0.40.5 on PATH (or embedded via gemma runtime)",
    ):
        print(f"- {line}")
    return 0


MODES = {"probe": lambda: probe(), "domains": domains}


def main(argv: list[str]) -> int:
    if len(argv) < 2 or argv[1] in {"-h", "--help", "help"}:
        print("usage: local-model-kit <probe|ask|domains> [prompt...]")
        return 2
    mode = argv[1]
    if mode == "ask":
        prompt = " ".join(argv[2:]).strip() or "Say hello in one short sentence."
        return ask(prompt)
    fn = MODES.get(mode)
    if fn is None:
        print(f"unknown mode: {mode}", file=sys.stderr)
        return 2
    return fn()


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
