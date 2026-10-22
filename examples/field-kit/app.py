#!/usr/bin/env python3
"""Field kit - stdlib-only modes that show adjacent uses of a single .de file.

No third-party deps. Each mode prints something useful offline so CI can assert
on stdout without a network, a GPU, or Ollama.

Modes:
  handoff     USB / courier delivery checklist (publisher -> recipient)
  inventory   Offline asset / SBOM-style inventory of the process environment
  classroom   Lab machine bootstrap card (one file per student machine)
  incident    Air-gapped incident-response helper card
  update      Air-gap SISR / delta update card (chunks on removable media)
  domains     List the adjacent domains this kit is meant to illustrate
"""

from __future__ import annotations

import json
import os
import platform
import sys
from datetime import datetime, timezone


def _now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def handoff() -> int:
    """Publisher -> recipient steps that match the real trust model."""
    print("=== FIELD KIT: HAND-OFF ===")
    print("1. Publisher: daedalus keygen --key-dir ./keys")
    print("2. Publisher: daedalus build . -o kit.de --key ./keys/<id>.key")
    print("3. Put kit.de AND the .pub on the same USB / courier package")
    print("4. Recipient: daedalus trust /media/usb/<id>.pub")
    print("5. Recipient: daedalus verify kit.de   # fails if trust is empty")
    print("6. Recipient: ./kit.de inventory")
    print(f"generated_at={_now()}")
    return 0


def inventory() -> int:
    """Offline inventory: what the host looks like when the kit starts."""
    payload = {
        "generated_at": _now(),
        "hostname": platform.node(),
        "platform": platform.platform(),
        "python": platform.python_version(),
        "pid": os.getpid(),
        "cwd": os.getcwd(),
        "env_keys": sorted(
            k
            for k in os.environ
            if k.startswith("DAEDALUS_") or k in {"HOME", "PATH", "LANG", "PORT"}
        ),
        "note": "stdlib only - no network call",
    }
    print("=== FIELD KIT: INVENTORY ===")
    print(json.dumps(payload, indent=2, sort_keys=True))
    return 0


def classroom() -> int:
    print("=== FIELD KIT: CLASSROOM ===")
    print("Give each student one kit.de. No pip, no apt, no Docker daemon.")
    print("Teacher keeps the .pub; students run: daedalus trust teacher.pub")
    print("Then: ./kit.de inventory   # proves the file runs on their laptop")
    print(f"generated_at={_now()}")
    return 0


def incident() -> int:
    print("=== FIELD KIT: INCIDENT ===")
    print("Air-gapped IR kit: ship tools as one ELF, verify before exec.")
    print("Keep the private key OFF the USB. Only the .pub rides with kit.de.")
    print("After use: daedalus clean --force on the analyst machine.")
    print(f"generated_at={_now()}")
    return 0


def update() -> int:
    """Air-gap delta path: SISR chunks ride the USB, not a registry pull."""
    print("=== FIELD KIT: UPDATE (SISR / air-gap) ===")
    print("1. Publisher: daedalus keygen --key-dir ./keys")
    print("2. Publisher: build kit-v1.de then kit-v2.de with --enable-sisr --key")
    print("3. Publisher: daedalus media stage kit-v2.de -o /media/usb/update \\")
    print("     --against kit-v1.de --pubkey ./keys/<id>.pub")
    print("4. Check: daedalus media verify /media/usb/update --against kit-v1.de")
    print("5. USB carries manifest + chunks/ + .pub (private key OFF media)")
    print("6. Recipient: daedalus trust <id>.pub  # empty/foreign trust fails closed")
    print("7. Recipient: DAEDALUS_SISR_MANIFEST=/media/usb/update/manifest ./kit-v1.de")
    print("   (apply chunks offline; refuse unsigned / foreign keys)")
    print("Note: synthetic SISR fixtures in CI are not a substitute for a")
    print("real chunk chain against a prior production artifact.")
    print(f"generated_at={_now()}")
    return 0


def domains() -> int:
    print("=== FIELD KIT: ADJACENT DOMAINS ===")
    for line in (
        "usb-courier: signed single-file delivery without a registry",
        "airgap-ops: updates via SISR chunks on removable media",
        "classroom-lab: identical runtime on every student machine",
        "incident-response: verified tooling where outbound net is blocked",
        "edge-fleet: same artifact on x64/arm with per-arch builds",
        "ci-provenance: attest + verify the SBOM of what you shipped",
        "local-ai: only when weights are actually inside the payload",
    ):
        print(f"- {line}")
    return 0


MODES = {
    "handoff": handoff,
    "inventory": inventory,
    "classroom": classroom,
    "incident": incident,
    "update": update,
    "domains": domains,
}

_USAGE = "usage: field-kit <handoff|inventory|classroom|incident|update|domains>"


def main(argv: list[str]) -> int:
    if len(argv) < 2 or argv[1] in {"-h", "--help", "help"}:
        print(_USAGE)
        return 2
    mode = argv[1]
    fn = MODES.get(mode)
    if fn is None:
        print(f"unknown mode: {mode}", file=sys.stderr)
        print(_USAGE)
        return 2
    return fn()


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
