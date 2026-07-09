"""Dynamic library (.so) detection via ldd.

`ldd` already resolves transitively, so a single pass gets all the required
.so files + the dynamic loader (ld-linux).
"""

from __future__ import annotations

import re
import subprocess
from pathlib import Path

# "libfoo.so.1 => /usr/lib/x86_64-linux-gnu/libfoo.so.1 (0x00007f...)"
_ARROW = re.compile(r"=>\s+(/\S+)\s+\(0x[0-9a-f]+\)")
# "/lib64/ld-linux-x86-64.so.2 (0x00007f...)"  (le loader, sans =>)
_DIRECT = re.compile(r"^\s*(/\S+)\s+\(0x[0-9a-f]+\)")


def shared_libs(binary: Path) -> set[Path]:
    """Return the set of absolute .so paths that `binary` depends on.

    Includes the dynamic loader (ld-linux). Returns an empty set if the binary
    is static or if ldd fails (we don't crash the build for this).
    """
    try:
        out = subprocess.run(
            ["ldd", str(binary)],
            capture_output=True,
            text=True,
            check=False,
        ).stdout
    except (FileNotFoundError, OSError):
        return set()

    libs: set[Path] = set()
    for line in out.splitlines():
        if "not a dynamic executable" in line or "statically linked" in line:
            continue
        m = _ARROW.search(line)
        if m:
            libs.add(Path(m.group(1)))
            continue
        m = _DIRECT.match(line)
        if m:
            libs.add(Path(m.group(1)))
    # Only keep paths that actually exist on disk.
    return {p for p in libs if p.exists()}
