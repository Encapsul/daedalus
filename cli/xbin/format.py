"""Spec du format .xbin — DOIT rester synchronisé avec stub/src/format.rs.

Voir docs/FORMAT.md. Le footer fait 84 bytes en fin de fichier.
"""

from __future__ import annotations

import struct
from dataclasses import dataclass

MAGIC = b"XBIN\x01"
FOOTER_MAGIC = 0xBEEFCAFE
FORMAT_VERSION = 2
FOOTER_SIZE = 84

# little-endian, sans padding :
#  5s  magic
#  B   format_version
#  B   arch
#  B   flags
#  Q   payload_offset
#  Q   payload_csize
#  Q   payload_usize
#  32s payload_sha256
#  Q   meta_offset
#  Q   meta_size
#  I   footer_magic
_FOOTER_FMT = "<5sBBBQQQ32sQQI"
assert struct.calcsize(_FOOTER_FMT) == FOOTER_SIZE, struct.calcsize(_FOOTER_FMT)

ARCH_X86_64 = 0x01
ARCH_AARCH64 = 0x02

FLAG_SIGNED = 0x01
FLAG_ENCRYPTED = 0x02


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

    def pack(self) -> bytes:
        return struct.pack(
            _FOOTER_FMT,
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

    @classmethod
    def unpack(cls, data: bytes) -> "Footer":
        if len(data) != FOOTER_SIZE:
            raise ValueError(f"footer must be {FOOTER_SIZE} bytes, got {len(data)}")
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
        ) = struct.unpack(_FOOTER_FMT, data)
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
        )


def read_footer(path: str) -> Footer:
    """Lit le footer en fin de fichier .xbin."""
    with open(path, "rb") as f:
        f.seek(-FOOTER_SIZE, 2)
        return Footer.unpack(f.read(FOOTER_SIZE))


ARCH_NAMES = {ARCH_X86_64: "x86_64", ARCH_AARCH64: "aarch64"}
