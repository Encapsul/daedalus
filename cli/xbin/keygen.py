from __future__ import annotations

import argparse
from pathlib import Path

from . import crypto


def keygen(args: argparse.Namespace) -> None:
    key_dir = Path(args.key_dir).expanduser().resolve()
    key_dir.mkdir(parents=True, exist_ok=True)

    fp = crypto.keygen(str(key_dir))
    print(fp)

    if not args.quiet:
        print("[xbin] Ed25519 keypair generated", file=__import__('sys').stderr)
        print(f"[xbin]   key:  {key_dir / f'{fp}.key'}", file=__import__('sys').stderr)
        print(f"[xbin]   pub:  {key_dir / f'{fp}.pub'}", file=__import__('sys').stderr)
        print(f"[xbin]   fingerprint: {fp}", file=__import__('sys').stderr)
