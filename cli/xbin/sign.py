from __future__ import annotations

import argparse
import hashlib
import os
import sys

from . import crypto
from . import format as fmt

_NO_KEYS_MSG = (
    "no .key files found in ~/.xbin/keys/; use --key or run 'xbin keygen' first"
)


def _resolve_signing_key(explicit_key: str | None) -> str:
    """Return the path to the Ed25519 private key to use for signing."""
    if explicit_key:
        return os.path.abspath(explicit_key)

    keys_dir = os.path.expanduser("~/.xbin/keys")
    try:
        entries = sorted(os.listdir(keys_dir))
    except FileNotFoundError:
        raise ValueError(_NO_KEYS_MSG) from None

    key_files = [e for e in entries if e.endswith(".key")]
    if not key_files:
        raise ValueError(_NO_KEYS_MSG)

    return os.path.join(keys_dir, key_files[0])


def sign(args: argparse.Namespace) -> None:
    path = os.path.abspath(args.file)
    footer = fmt.read_footer(path)
    if footer.flags & fmt.FLAG_SIGNED:
        print(f"[xbin] warning: {path} is already signed; re-signing", file=sys.stderr)

    with open(path, "rb") as f:
        payload = fmt.read_at(f, footer.payload_offset, footer.payload_csize)
        meta = fmt.read_at(f, footer.meta_offset, footer.meta_size)

    body_hash = hashlib.sha256(payload + meta).digest()

    key_path = _resolve_signing_key(args.key)
    if not os.path.isfile(key_path):
        raise ValueError(f"key file not found: {key_path}")

    sig = crypto.sign(key_path, body_hash)
    if len(sig) != fmt.SIG_BLOCK_SIZE_FIELD:
        raise ValueError(
            f"expected {fmt.SIG_BLOCK_SIZE_FIELD}-byte signature, got {len(sig)}"
        )

    sig_block = fmt.pack_sig_block(sig)
    sig_offset = _write_signed(path, footer, sig_block)

    if not args.quiet:
        print(f"[xbin] signed {path} with {key_path}", file=sys.stderr)
        print(f"[xbin]   signature block at offset {sig_offset}", file=sys.stderr)


def _write_signed(path: str, footer: fmt.Footer, sig_block: bytes) -> int:
    """Replace the footer with a v3 footer containing the signature block.

    Returns the file offset where the signature block was written.
    """
    old_footer_size = footer.footer_size
    total = os.path.getsize(path)

    with open(path, "r+b") as f:
        new_size = total - old_footer_size
        f.truncate(new_size)
        f.seek(new_size)

        sig_offset = f.tell()
        f.write(sig_block)

        footer.format_version = 3
        footer.flags |= fmt.FLAG_SIGNED
        footer.sig_offset = sig_offset
        f.write(footer.pack())

    return sig_offset
