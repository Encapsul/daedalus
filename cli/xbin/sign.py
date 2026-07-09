from __future__ import annotations

import argparse
import hashlib
import os
import struct
import sys

from . import crypto, format as fmt


def sign(args: argparse.Namespace) -> None:
    path = os.path.abspath(args.file)

    # 1. Read footer and locate payload + metadata.
    footer = fmt.read_footer(path)
    if footer.flags & fmt.FLAG_SIGNED:
        print(f"[xbin] warning: {path} is already signed; re-signing", file=sys.stderr)

    # 2. Read payload and metadata.
    with open(path, "rb") as f:
        payload = fmt.read_at(f, footer.payload_offset, footer.payload_csize)
        meta = fmt.read_at(f, footer.meta_offset, footer.meta_size)

    # 3. Compute SHA-256(payload ‖ meta).
    body_hash = hashlib.sha256(payload + meta).digest()

    # 4. Find signing key file (--key or derive from first key in ~/.xbin/keys/).
    if args.key:
        key_path = os.path.abspath(args.key)
    else:
        keys_dir = os.path.expanduser("~/.xbin/keys")
        entries = sorted(os.listdir(keys_dir))
        key_files = [e for e in entries if e.endswith(".key")]
        if not key_files:
            print("[xbin] error: no .key files found in ~/.xbin/keys/; "
                  "use --key or run 'xbin keygen' first", file=sys.stderr)
            sys.exit(1)
        key_path = os.path.join(keys_dir, key_files[0])

    if not os.path.isfile(key_path):
        print(f"[xbin] error: key file not found: {key_path}", file=sys.stderr)
        sys.exit(1)

    # 5. Sign.
    sig = crypto.sign(key_path, body_hash)
    if len(sig) != 64:
        print(f"[xbin] error: expected 64-byte signature, got {len(sig)}", file=sys.stderr)
        sys.exit(1)

    sig_block = struct.pack("<I", 64) + sig  # 68 bytes

    # 6. Truncate footer and append sig_block + v3 footer.
    old_footer_size = footer.footer_size
    total = os.path.getsize(path)

    with open(path, "r+b") as f:
        # Truncate at the start of the old footer.
        new_size = total - old_footer_size
        f.truncate(new_size)
        f.seek(new_size)

        # Write signature block.
        sig_offset = f.tell()
        f.write(sig_block)

        # Write v3 footer.
        footer.format_version = 3
        footer.flags |= fmt.FLAG_SIGNED
        footer.sig_offset = sig_offset
        f.write(footer.pack())

    if not args.quiet:
        pub_path = key_path.replace(".key", ".pub")
        print(f"[xbin] signed {path} with {key_path}", file=sys.stderr)
        print(f"[xbin]   signature block at offset {sig_offset}", file=sys.stderr)
