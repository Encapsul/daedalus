""".xbin format spec — MUST stay in sync with stub/src/format.rs.

Footer versions:
  v1/v2 — 84 bytes at EOF-84.
  v3    — 92 bytes at EOF-92.  The last 84 bytes are byte-identical to v2,
          so a v2 launcher reading EOF-84 sees the correct magic + format_version
          and reports "unsupported format" cleanly.  A v3-aware reader reads 92
          bytes and picks sig_offset from the 8-byte prefix.

Layout of the 92-byte v3 footer (little-endian):
  [0-7]    sig_offset (u64)          offset of [sig_size:u32le][sig:64 bytes]
  [8-12]   magic (5 bytes)           "XBIN\x01"
  [13]     format_version (u8)       3
  [14]     arch (u8)
  [15]     flags (u8)                bit0=signed
  [16-23]  payload_offset (u64)
  [24-31]  payload_csize (u64)
  [32-39]  payload_usize (u64)       unused in v2/v3 (per-layer sizes in metadata)
  [40-71]  payload_sha256 (32 bytes) SHA-256(layers ‖ metadata)
  [72-79]  meta_offset (u64)
  [80-87]  meta_size (u64)
  [88-91]  footer_magic (u32)        0xBEEFCAFE
"""

from __future__ import annotations

import struct
from dataclasses import dataclass

MAGIC = b"XBIN\x01"
FOOTER_MAGIC = 0xBEEFCAFE
FORMAT_VERSION = 3
V2_FOOTER_SIZE = 84
V3_FOOTER_SIZE = 92

# little-endian pack/unpack for the 84-byte core (identical across all versions):
#   5s  magic
#   B   format_version
#   B   arch
#   B   flags
#   Q   payload_offset
#   Q   payload_csize
#   Q   payload_usize
#   32s payload_sha256
#   Q   meta_offset
#   Q   meta_size
#   I   footer_magic
_CORE_FMT = "<5sBBBQQQ32sQQI"
assert struct.calcsize(_CORE_FMT) == V2_FOOTER_SIZE, struct.calcsize(_CORE_FMT)

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
    sig_offset: int = 0  # v3+: offset of signature block; 0 for v1/v2

    @property
    def footer_size(self) -> int:
        return V3_FOOTER_SIZE if self.format_version >= 3 else V2_FOOTER_SIZE

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
            return struct.pack("<Q", self.sig_offset) + core  # 92 bytes
        return core  # 84 bytes

    @classmethod
    def unpack(cls, data: bytes) -> "Footer":
        sig_offset = 0
        if len(data) == V3_FOOTER_SIZE:
            sig_offset = struct.unpack_from("<Q", data, 0)[0]
            data = data[8:]  # strip prefix → 84-byte core
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


def read_footer(path: str) -> Footer:
    """Read the footer at the end of a .xbin file.

    Detection order: v3 footer (92 bytes @ EOF-92) → v2 footer (84 bytes @ EOF-84).
    """
    with open(path, "rb") as f:
        total = f.seek(0, 2)  # EOF

        # Try v3/v2 footer (92 bytes from EOF).
        if total >= V3_FOOTER_SIZE:
            f.seek(-V3_FOOTER_SIZE, 2)
            buf = f.read(V3_FOOTER_SIZE)
            if buf[8:13] == MAGIC:
                return Footer.unpack(buf)

        # Fallback to v1/v2 footer (84 bytes from EOF).
        if total >= V2_FOOTER_SIZE:
            f.seek(-V2_FOOTER_SIZE, 2)
            return Footer.unpack(f.read(V2_FOOTER_SIZE))

        raise ValueError("file too small to be a .xbin")


ARCH_NAMES = {ARCH_X86_64: "x86_64", ARCH_AARCH64: "aarch64"}


def read_at(f, offset: int, length: int) -> bytes:
    """Read `length` bytes at absolute offset `offset`."""
    f.seek(offset)
    return f.read(length)
