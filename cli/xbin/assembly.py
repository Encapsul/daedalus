"""xbin assembly: metadata, signing, and binary assembly.

Extracted from build.py to keep each file under 300 lines.
"""

from __future__ import annotations

import hashlib
import json
import os
import platform
from datetime import UTC, datetime
from pathlib import Path

from . import _format as fmt
from . import crypto

XBIN_VERSION = "0.1.0"


def build_meta_json(
    *,
    name: str,
    runtime: str,
    isolation: int,
    entrypoint: list[str],
    env: dict[str, str],
    layers: list[dict],
    services: list[dict] | None = None,
    seccomp: bool = False,
    crypto: dict | None = None,
    payload_format: str = "",
    app_hash: str = "",
    rt_deps_hash: str = "",
    version: str = "",
    author: str = "",
    description: str = "",
    license: str = "",
) -> bytes:
    """Build the metadata JSON bytes for the .xbin footer."""
    meta: dict = {
        "name": name,
        "xbin_version": XBIN_VERSION,
        "created": datetime.now(UTC).isoformat(),
        "runtime": runtime,
        "isolation": isolation,
        "entrypoint": entrypoint,
        "env": env,
        "layers": layers,
    }
    if version:
        meta["version"] = version
    if author:
        meta["author"] = author
    if description:
        meta["description"] = description
    if license:
        meta["license"] = license
    if payload_format:
        meta["payload_format"] = payload_format
    if seccomp:
        meta["seccomp"] = True
    if crypto:
        meta["crypto"] = crypto
    if services:
        meta["services"] = services
    if app_hash:
        meta["app_hash"] = app_hash
    if rt_deps_hash:
        meta["rt_deps_hash"] = rt_deps_hash
    return json.dumps(meta, separators=(",", ":")).encode()


def sign_and_write(
    f, footer: fmt.Footer, key_path: str, payload: bytes, meta_bytes: bytes
) -> None:
    """Sign payload+meta and write sig_block + updated footer to an open file.

    Updates footer.format_version, flags, and sig_offset in-place.
    """
    body_hash = hashlib.sha256(payload + meta_bytes).digest()
    sig = crypto.sign(key_path, body_hash)
    sig_block = fmt.pack_sig_block(sig)
    footer.format_version = 3
    footer.flags |= fmt.FLAG_SIGNED
    footer.sig_offset = f.tell()
    f.write(sig_block)
    f.write(footer.pack())


def assemble_xbin(
    out_path: Path,
    stub: Path,
    payload: bytes,
    meta_bytes: bytes,
    key_path: str | None,
    encrypt: bool = False,
    squashfs: bool = False,
    target_arch: str | None = None,
) -> int:
    """Write [stub][payload][metadata][optional sig+footer] to disk.

    Returns the total file size.
    """
    stub_bytes = stub.read_bytes()
    # v5 when squashfs (payload_format in metadata), v4 when encrypting,
    # v3 when signing, v2 otherwise.
    if squashfs:
        fmt_ver = 5
    elif encrypt:
        fmt_ver = 4
    elif key_path:
        fmt_ver = 3
    else:
        fmt_ver = 2
    # Determine arch: use target_arch if cross-building, else host.
    if target_arch:
        arch = fmt.ARCH_AARCH64 if target_arch == "aarch64" else fmt.ARCH_X86_64
    else:
        arch = fmt.ARCH_AARCH64 if platform.machine() == "aarch64" else fmt.ARCH_X86_64
    footer = fmt.Footer(
        format_version=fmt_ver,
        arch=arch,
        flags=0,
        payload_offset=len(stub_bytes),
        payload_csize=len(payload),
        payload_usize=fmt.CRYPTO_AES_256_GCM if encrypt else 0,
        payload_sha256=hashlib.sha256(payload + meta_bytes).digest(),
        meta_offset=len(stub_bytes) + len(payload),
        meta_size=len(meta_bytes),
    )
    with open(out_path, "wb") as f:
        f.write(stub_bytes)
        f.write(payload)
        f.write(meta_bytes)
        if key_path:
            sign_and_write(f, footer, key_path, payload, meta_bytes)
        else:
            f.write(footer.pack())
    os.chmod(out_path, 0o755)
    return out_path.stat().st_size


def read_signing_seed(key_path: str) -> bytes:
    """Read a 32-byte Ed25519 signing seed from a .key file."""
    seed = Path(key_path).read_bytes()
    if len(seed) != 32:
        raise ValueError(f"signing key must be 32 bytes, got {len(seed)}")
    return seed
