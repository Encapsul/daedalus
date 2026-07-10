from __future__ import annotations

import argparse
import hashlib
import os
import struct
import sys

from . import crypto, format as fmt


def verify(args: argparse.Namespace) -> None:
    path = os.path.abspath(args.file)

    footer = fmt.read_footer(path)
    if not (footer.flags & fmt.FLAG_SIGNED):
        print(f"[xbin] {path} is not signed", file=sys.stderr)
        sys.exit(1)

    with open(path, "rb") as f:
        payload = fmt.read_at(f, footer.payload_offset, footer.payload_csize)
        meta = fmt.read_at(f, footer.meta_offset, footer.meta_size)

    body_hash = hashlib.sha256(payload + meta).digest()

    # Read signature block.
    with open(path, "rb") as f:
        sig_data = fmt.read_at(f, footer.sig_offset, 68)
    sig_size = struct.unpack_from("<I", sig_data, 0)[0]
    if sig_size != 64:
        print(f"[xbin] error: unexpected signature size {sig_size}", file=sys.stderr)
        sys.exit(2)
    sig = sig_data[4:68]

    hash_and_sig = body_hash + sig  # 96 bytes

    # Determine trusted keys directory.
    trusted_dir = args.trusted_dir
    if trusted_dir:
        trusted_dir = os.path.abspath(trusted_dir)
    else:
        trusted_dir = os.path.expanduser("~/.xbin/trusted-keys")

    if not os.path.isdir(trusted_dir):
        print(f"[xbin] error: trusted keys directory not found: {trusted_dir}",
              file=sys.stderr)
        sys.exit(2)

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
        sys.exit(0)
    else:
        print(f"[xbin] signature verification FAILED for {path}", file=sys.stderr)
        sys.exit(1)
