from __future__ import annotations

import argparse
import hashlib
import os
import sys

from . import crypto
from . import format as fmt


def verify(args: argparse.Namespace) -> None:
    path = os.path.abspath(args.file)

    footer = fmt.read_footer(path)
    if not (footer.flags & fmt.FLAG_SIGNED):
        raise ValueError(f"{path} is not signed")

    with open(path, "rb") as f:
        payload = fmt.read_at(f, footer.payload_offset, footer.payload_csize)
        meta = fmt.read_at(f, footer.meta_offset, footer.meta_size)

    body_hash = hashlib.sha256(payload + meta).digest()

    with open(path, "rb") as f:
        sig_data = fmt.read_at(f, footer.sig_offset, fmt.SIG_BLOCK_SIZE)
    sig = fmt.unpack_sig_block(sig_data)

    hash_and_sig = body_hash + sig  # 96 bytes

    trusted_dir = args.trusted_dir
    if trusted_dir:
        trusted_dir = os.path.abspath(trusted_dir)
    else:
        trusted_dir = os.path.expanduser("~/.xbin/trusted-keys")

    if not os.path.isdir(trusted_dir):
        raise ValueError(f"trusted keys directory not found: {trusted_dir}")

    verified = False
    for entry in sorted(os.listdir(trusted_dir)):
        pubkey_path = os.path.join(trusted_dir, entry)
        if not os.path.isfile(pubkey_path):
            continue
        try:
            rc = crypto.verify(pubkey_path, hash_and_sig)
            if rc == 0:
                verified = True
                break
        except RuntimeError:
            continue

    if verified:
        if not args.quiet:
            print(f"[xbin] signature verified for {path}", file=sys.stderr)
    else:
        raise ValueError(f"signature verification FAILED for {path}")
