"""Pure-Python ELF analyzer — no dependency on host `ldd`.

Parses ELF64 headers, reads DT_NEEDED entries, and resolves shared library
paths by searching standard system directories. Works on any platform that
can run Python (Linux, macOS, Windows WSL).
"""

from __future__ import annotations

import platform
import struct
from pathlib import Path

# ELF64 constants
EI_CLASS = 4
EI_DATA = 5
ELFCLASS64 = 2
ELFDATA2LSB = 1
ET_DYN = 3
ET_EXEC = 2

# Program header types
PT_NULL = 0
PT_LOAD = 1
PT_DYNAMIC = 2
PT_INTERP = 3

# Dynamic entry types
DT_NULL = 0
DT_NEEDED = 1
DT_STRTAB = 5
DT_RUNPATH = 29
DT_RPATH = 15

# Architecture-specific GNU triplet and standard search paths.
_MACHINE = platform.machine()
_GNU_TRIPLET = "aarch64-linux-gnu" if _MACHINE == "aarch64" else "x86_64-linux-gnu"

_STANDARD_PATHS = [
    f"/lib/{_GNU_TRIPLET}",
    f"/usr/lib/{_GNU_TRIPLET}",
    "/lib64",
    "/lib",
    "/usr/lib64",
    "/usr/lib",
]


class ELFParser:
    def __init__(self, path: Path):
        self.path = path
        self.data = path.read_bytes()
        self._validate()

    def _validate(self):
        if self.data[:4] != b"\x7fELF":
            raise ValueError(f"not an ELF file: {self.path}")
        if self.data[EI_CLASS] != ELFCLASS64:
            raise ValueError(f"only ELF64 supported: {self.path}")
        if self.data[EI_DATA] != ELFDATA2LSB:
            raise ValueError(f"only little-endian ELF supported: {self.path}")

    # ------------------------------------------------------------------
    # ELF header fields
    # ------------------------------------------------------------------
    @property
    def e_type(self) -> int:
        return struct.unpack_from("<H", self.data, 16)[0]

    @property
    def e_phoff(self) -> int:
        return struct.unpack_from("<Q", self.data, 32)[0]

    @property
    def e_phentsize(self) -> int:
        return struct.unpack_from("<H", self.data, 54)[0]

    @property
    def e_phnum(self) -> int:
        return struct.unpack_from("<H", self.data, 56)[0]

    # ------------------------------------------------------------------
    # Program headers
    # ------------------------------------------------------------------
    def _program_headers(self):
        for i in range(self.e_phnum):
            off = self.e_phoff + i * self.e_phentsize
            (
                p_type,
                p_flags,
                p_offset,
                p_vaddr,
                _p_paddr,
                p_filesz,
                p_memsz,
                _p_align,
            ) = struct.unpack_from("<IIQQQQQQ", self.data, off)
            yield {
                "type": p_type,
                "flags": p_flags,
                "offset": p_offset,
                "vaddr": p_vaddr,
                "filesz": p_filesz,
                "memsz": p_memsz,
            }

    def _interp(self) -> str | None:
        for ph in self._program_headers():
            if ph["type"] == PT_INTERP:
                raw = self.data[ph["offset"] : ph["offset"] + ph["filesz"]]
                return raw.rstrip(b"\x00").decode("utf-8", errors="replace")
        return None

    def _dynamic_section(self) -> dict | None:
        for ph in self._program_headers():
            if ph["type"] == PT_DYNAMIC:
                return {
                    "vaddr": ph["vaddr"],
                    "offset": ph["offset"],
                    "size": ph["memsz"],
                }
        return None

    # ------------------------------------------------------------------
    # Dynamic entries
    # ------------------------------------------------------------------
    def _parse_dynamic(self) -> tuple[list[str], list[str], str | None]:
        """Return (needed_libs, runpaths, strtab_addr)."""
        dyn = self._dynamic_section()
        if not dyn:
            return [], [], None

        needed: list[str] = []
        runpaths: list[str] = []
        strtab_addr: int | None = None

        off = dyn["offset"]
        size = dyn["size"]
        end = off + size
        pos = off
        while pos + 16 <= end:
            d_tag, d_val = struct.unpack_from("<qQ", self.data, pos)
            if d_tag == DT_NULL:
                break
            elif d_tag == DT_NEEDED:
                needed.append(str(d_val))  # placeholder, resolved via strtab
            elif d_tag == DT_STRTAB:
                strtab_addr = d_val
            elif d_tag in (DT_RUNPATH, DT_RPATH):
                runpaths.append(str(d_val))
            pos += 16

        # Resolve DT_NEEDED names via strtab
        if strtab_addr is not None:
            resolved: list[str] = []
            for s_off_str in needed:
                s_off = int(s_off_str)
                # Find strtab offset: strtab_vaddr - dyn_vaddr + dyn_offset
                s_addr = strtab_addr + s_off
                # Convert vaddr to file offset
                s_file_off = self._vaddr_to_offset(s_addr)
                if s_file_off is not None:
                    name = self._read_str(s_file_off)
                    if name:
                        resolved.append(name)
            needed = resolved

            # Resolve RUNPATH/RPATH strings.
            # $ORIGIN is a dynamic linker token meaning "the directory
            # containing this ELF binary" — same semantics as ld.so.
            origin = str(self.path.parent)
            resolved_rp: list[str] = []
            for rp_off_str in runpaths:
                rp_off = int(rp_off_str)
                s_addr = strtab_addr + rp_off
                s_file_off = self._vaddr_to_offset(s_addr)
                if s_file_off is not None:
                    name = self._read_str(s_file_off)
                    if name:
                        resolved_rp.append(name.replace("$ORIGIN", origin))
            runpaths = resolved_rp

        return needed, runpaths, strtab_addr

    def _vaddr_to_offset(self, vaddr: int) -> int | None:
        for ph in self._program_headers():
            if ph["type"] == PT_LOAD:
                if ph["vaddr"] <= vaddr < ph["vaddr"] + ph["memsz"]:
                    return ph["offset"] + (vaddr - ph["vaddr"])
        return None

    def _read_str(self, off: int) -> str:
        end = off
        while end < len(self.data) and self.data[end] != 0:
            end += 1
        return self.data[off:end].decode("utf-8", errors="replace")

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------
    def shared_libs(self) -> set[Path]:
        """Return the set of absolute paths of shared libraries needed by
        this ELF binary. Recursively resolves dependencies.

        Includes the dynamic linker (PT_INTERP). Symlinks are preserved
        so that _copy_into_rootfs can recreate them in the rootfs.
        Only returns paths that actually exist on disk.
        """
        needed, runpaths, _ = self._parse_dynamic()
        found: set[Path] = set()

        # Include the dynamic linker (PT_INTERP) — may be a symlink on the
        # build host (e.g. /lib64/ld-linux-x86-64.so.2).  Preserving it lets
        # _copy_into_rootfs recreate the symlink chain.
        interp = self._interp()
        if interp:
            ip = Path(interp)
            if ip.exists():
                found.add(ip)

        if not needed:
            return found

        search_dirs = self._search_dirs(runpaths)
        _resolve_recursive(needed, search_dirs, found)

        # Deduplicate: remove resolved real-file paths that are the target of
        # a symlink already in the set.  This avoids having both the symlink
        # (e.g. /lib64/ld-linux-x86-64.so.2 → ../lib/.../ld-linux-x86-64.so.2)
        # and the real file (e.g. /lib/.../ld-linux-x86-64.so.2) in the result.
        # We keep the symlink so _copy_into_rootfs recreates it in the rootfs.
        targets: set[Path] = set()
        for p in found:
            if p.is_symlink():
                try:
                    targets.add(p.resolve())
                except (OSError, RuntimeError):
                    pass
        kept: set[Path] = set()
        for p in found:
            if p.is_symlink() or p.resolve() not in targets:
                kept.add(p)
        found = kept

        return found

    def _search_dirs(self, extra_rpaths: list[str]) -> list[Path]:
        dirs: list[Path] = []
        for rp in extra_rpaths:
            dirs.append(Path(rp))
        for p in _STANDARD_PATHS:
            dirs.append(Path(p))
        return dirs

    def interpreter(self) -> str | None:
        """Return the dynamic linker path (PT_INTERP), or None."""
        return self._interp()


def _resolve_recursive(
    names: list[str],
    search_dirs: list[Path],
    found: set[Path],
) -> None:
    """Resolve DT_NEEDED entries, using each library's own runpaths for its deps."""
    seen_names: set[str] = set()
    # Each queue entry carries its own search_dirs (inherited from the parent
    # that loaded it, plus the library's own DT_RUNPATH/DT_RPATH).
    queue: list[tuple[str, list[Path]]] = [(n, search_dirs) for n in names]
    while queue:
        name, dirs = queue.pop(0)
        if name in seen_names:
            continue
        seen_names.add(name)
        resolved = _find_lib(name, dirs)
        if resolved and resolved not in found:
            found.add(resolved)
            try:
                sub = ELFParser(resolved)
                sub_needed, sub_runpaths, _ = sub._parse_dynamic()
                sub_dirs = sub._search_dirs(sub_runpaths)
                for n in sub_needed:
                    if n not in seen_names:
                        queue.append((n, sub_dirs))
            except (ValueError, OSError):
                pass


def _find_lib(name: str, search_dirs: list[Path]) -> Path | None:
    """Find a shared library by name in the search directories.

    Returns the symlink path (e.g. ``/usr/lib/libz.so.1``) rather than the
    resolved real file (``libz.so.1.3``) so that ``_copy_into_rootfs`` can
    recreate the SONAME symlink inside the rootfs.
    """
    for d in search_dirs:
        candidate = d / name
        if candidate.is_file():
            return candidate
    return None


def shared_libs(binary: Path) -> set[Path]:
    """Public convenience wrapper.

    Returns the set of .so paths needed by `binary`.
    Falls back to ldd-based detection if ELF parsing fails.
    """
    try:
        parser = ELFParser(binary)
        return parser.shared_libs()
    except (ValueError, OSError, PermissionError):
        # Fallback to ldd
        return _fallback_ldd(binary)


def _fallback_ldd(binary: Path) -> set[Path]:
    """Fallback: use system `ldd`."""
    import subprocess
    import re

    _ARROW = re.compile(r"=>\s+(/\S+)\s+\(0x[0-9a-f]+\)")
    _DIRECT = re.compile(r"^\s*(/\S+)\s+\(0x[0-9a-f]+\)")

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
    return {p for p in libs if p.exists()}
