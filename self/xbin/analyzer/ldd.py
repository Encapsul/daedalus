"""Dynamic library (.so) detection.

Primary: pure-Python ELF parser (cross-platform, no host `ldd` needed).
Fallback: system `ldd` command.

This module exists as a facade so the rest of the builder doesn't need to
know about ELF parsing or `ldd`.
"""

from __future__ import annotations

from pathlib import Path

from . import elf


def shared_libs(binary: Path) -> set[Path]:
    """Return the set of absolute .so paths that `binary` depends on.

    Includes the dynamic loader (ld-linux). Returns an empty set if the binary
    is static or if analysis fails (we don't crash the build for this).

    Uses pure-Python ELF parsing by default, with an automatic fallback
    to the host's `ldd` command when ELF parsing fails.
    """
    return elf.shared_libs(binary)
