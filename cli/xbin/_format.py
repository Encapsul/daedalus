""".xbin format wrapper using xbin-core PyO3 bindings.

This module provides the same API as format.py but delegates to xbin_core (Rust).
"""

from __future__ import annotations

import struct
from dataclasses import dataclass

try:
    import xbin_core

    _HAS_XBIN_CORE = True
except ImportError:
    _HAS_XBIN_CORE = False

# ─── Constants (kept in sync with xbin-core) ──────────────────────────────

MAGIC = b"XBIN\x01"
FOOTER_MAGIC = 0xBEEFCAFE
FORMAT_VERSION = 5
V2_FOOTER_SIZE = 84
V3_FOOTER_SIZE = 92

CRYPTO_NONE = 0x00
CRYPTO_AES_256_GCM = 0x01

PAYLOAD_FORMAT_ZSTD_TAR = "zstd-tar"
PAYLOAD_FORMAT_SQUASHFS = "squashfs"

_CORE_FMT = "<5sBBBQQQ32sQQI"
assert struct.calcsize(_CORE_FMT) == V2_FOOTER_SIZE

ARCH_X86_64 = 0x01
ARCH_AARCH64 = 0x02

FLAG_SIGNED = 0x01
FLAG_ENCRYPTED = 0x02

SIG_BLOCK_SIZE = 68
SIG_BLOCK_SIZE_FIELD = 64

ARCH_NAMES = {ARCH_X86_64: "x86_64", ARCH_AARCH64: "aarch64"}


# ─── Helper functions ─────────────────────────────────────────────────────


def pack_sig_block(sig: bytes) -> bytes:
    if len(sig) != SIG_BLOCK_SIZE_FIELD:
        raise ValueError(
            f"signature must be {SIG_BLOCK_SIZE_FIELD} bytes, got {len(sig)}"
        )
    return struct.pack("<I", SIG_BLOCK_SIZE_FIELD) + sig


def unpack_sig_block(data: bytes) -> bytes:
    if len(data) != SIG_BLOCK_SIZE:
        raise ValueError(f"sig_block must be {SIG_BLOCK_SIZE} bytes, got {len(data)}")
    sig_size = struct.unpack_from("<I", data, 0)[0]
    if sig_size != SIG_BLOCK_SIZE_FIELD:
        raise ValueError(f"unexpected sig_size field: {sig_size}")
    return data[4 : 4 + SIG_BLOCK_SIZE_FIELD]


# ─── Footer dataclass (Python-side, matches xbin_core PyFooter) ───────────


@dataclass
class Footer:
    format_version: int
    arch: int
    flags: int
    payload_offset: int
    payload_csize: int
    payload_usize: int
    payload_sha256: bytes
    meta_offset: int
    meta_size: int
    sig_offset: int = 0

    @property
    def footer_size(self) -> int:
        return V3_FOOTER_SIZE if self.format_version >= 3 else V2_FOOTER_SIZE

    @property
    def crypto_suite(self) -> int:
        if self.format_version >= 4:
            return self.payload_usize
        return CRYPTO_NONE

    @crypto_suite.setter
    def crypto_suite(self, value: int) -> None:
        if self.format_version >= 4:
            self.payload_usize = value

    def pack(self) -> bytes:
        core = struct.pack(
            _CORE_FMT,
            MAGIC,
            self.format_version,
            self.arch,
            self.flags,
            self.payload_offset,
            self.payload_csize,
            self.payload_usize,
            self.payload_sha256,
            self.meta_offset,
            self.meta_size,
            FOOTER_MAGIC,
        )
        if self.format_version >= 3:
            return struct.pack("<Q", self.sig_offset) + core
        return core

    @classmethod
    def unpack(cls, data: bytes) -> Footer:
        sig_offset = 0
        if len(data) == V3_FOOTER_SIZE:
            sig_offset = struct.unpack_from("<Q", data, 0)[0]
            data = data[8:]
        elif len(data) != V2_FOOTER_SIZE:
            raise ValueError(
                f"footer must be {V2_FOOTER_SIZE} or {V3_FOOTER_SIZE} bytes, "
                f"got {len(data)}"
            )

        (
            magic,
            format_version,
            arch,
            flags,
            payload_offset,
            payload_csize,
            payload_usize,
            payload_sha256,
            meta_offset,
            meta_size,
            footer_magic,
        ) = struct.unpack(_CORE_FMT, data)

        if magic != MAGIC:
            raise ValueError("bad magic: not a .xbin file")
        if footer_magic != FOOTER_MAGIC:
            raise ValueError("bad footer sentinel")

        return cls(
            format_version=format_version,
            arch=arch,
            flags=flags,
            payload_offset=payload_offset,
            payload_csize=payload_csize,
            payload_usize=payload_usize,
            payload_sha256=payload_sha256,
            meta_offset=meta_offset,
            meta_size=meta_size,
            sig_offset=sig_offset,
        )


# ─── I/O functions (delegate to xbin_core if available) ───────────────────


def read_footer(path: str) -> Footer:
    if _HAS_XBIN_CORE:
        pf = xbin_core.py_read_footer(path)
        return Footer(
            format_version=pf.format_version,
            arch=pf.arch,
            flags=pf.flags,
            payload_offset=pf.payload_offset,
            payload_csize=pf.payload_csize,
            payload_usize=pf.payload_usize,
            payload_sha256=bytes(pf.payload_sha256),
            meta_offset=pf.meta_offset,
            meta_size=pf.meta_size,
            sig_offset=pf.sig_offset,
        )
    # Fallback to pure Python implementation
    with open(path, "rb") as f:
        total = f.seek(0, 2)

        if total >= V3_FOOTER_SIZE:
            f.seek(-V3_FOOTER_SIZE, 2)
            buf = f.read(V3_FOOTER_SIZE)
            if buf[8:13] == MAGIC and buf[13] >= 3:
                return Footer.unpack(buf)

        if total >= V2_FOOTER_SIZE:
            f.seek(-V2_FOOTER_SIZE, 2)
            return Footer.unpack(f.read(V2_FOOTER_SIZE))

        raise ValueError("file too small to be a .xbin")


def read_at(f, offset: int, length: int) -> bytes:
    if _HAS_XBIN_CORE:
        return xbin_core.py_read_at(
            f.name if hasattr(f, "name") else "", offset, length
        )
    f.seek(offset)
    return f.read(length)
