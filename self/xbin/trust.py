from __future__ import annotations

import argparse
import hashlib
import os
import shutil
import sys
from pathlib import Path


def trust(args: argparse.Namespace) -> None:
    pubkey_path = Path(args.pubkey).expanduser().resolve()
    if not pubkey_path.is_file():
        print(f"[xbin] error: file not found: {pubkey_path}", file=sys.stderr)
        sys.exit(1)

    raw = pubkey_path.read_bytes()
    if len(raw) != 32:
        print(f"[xbin] error: public key must be exactly 32 bytes, got {len(raw)}",
              file=sys.stderr)
        sys.exit(1)

    dest_dir = Path(args.trusted_dir).expanduser().resolve()
    dest_dir.mkdir(parents=True, exist_ok=True)

    fp = hashlib.sha256(raw).hexdigest()
    dest_path = dest_dir / f"{fp}.pub"

    if dest_path.exists():
        print(f"[xbin] key {fp} already trusted at {dest_path}", file=sys.stderr)
        sys.exit(0)

    shutil.copy2(pubkey_path, dest_path)
    if not args.quiet:
        print(f"[xbin] trusted key {fp}", file=sys.stderr)
        print(f"[xbin]   copied {pubkey_path} -> {dest_path}", file=sys.stderr)
