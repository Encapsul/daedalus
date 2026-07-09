"""xbin build: analyze an app, build the rootfs, assemble the .xbin."""

from __future__ import annotations

import hashlib
import io
import json
import os
import shutil
import struct
import subprocess
import tarfile
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path

from . import crypto, format as fmt
from .analyzer import ldd, runtime

XBIN_VERSION = "0.1.0"


def find_stub() -> Path:
    """Locate the compiled launcher stub."""
    here = Path(__file__).resolve()
    repo = here.parents[2]  # cli/xbin/build.py -> repo root
    tmp_target = Path("/tmp/xbin-stub-target")
    candidates = [
        repo / "stub/target/x86_64-unknown-linux-musl/release/xbin-stub",
        repo / "stub/target/release/xbin-stub",
        tmp_target / "x86_64-unknown-linux-musl/release/xbin-stub",
        tmp_target / "release/xbin-stub",
    ]
    env = os.environ.get("XBIN_STUB")
    if env:
        candidates.insert(0, Path(env))
    for c in candidates:
        if c.is_file():
            return c
    raise FileNotFoundError(
        "launcher stub not found. Build it first:\n"
        "  cd stub && cargo build --release --target x86_64-unknown-linux-musl"
    )


def _copy_into_rootfs(host_path: Path, rootfs: Path) -> None:
    """Copy a host file into the rootfs, preserving its absolute path.

    E.g. /usr/lib/x86_64-linux-gnu/libc.so.6 -> rootfs/usr/lib/x86_64-linux-gnu/libc.so.6
    Resolves symlinks to real targets but re-creates the symlink itself.
    """
    rel = str(host_path).lstrip("/")
    dest = rootfs / rel
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.exists() or dest.is_symlink():
        return
    if host_path.is_symlink():
        # Recreate the symlink AND embed the target.
        target = os.readlink(host_path)
        real = host_path.resolve()
        _copy_into_rootfs(real, rootfs)
        try:
            dest.symlink_to(target)
        except FileExistsError:
            pass
    else:
        shutil.copy2(host_path, dest)


def _write_etc(rootfs: Path) -> None:
    """Minimal /etc for apps that need user/DNS resolution."""
    etc = rootfs / "etc"
    etc.mkdir(parents=True, exist_ok=True)
    (etc / "passwd").write_text("root:x:0:0:root:/root:/bin/sh\n")
    (etc / "group").write_text("root:x:0:\n")
    (etc / "nsswitch.conf").write_text("hosts: files dns\n")
    (etc / "resolv.conf").write_text("nameserver 1.1.1.1\n")


def _build_runtime_layer(app_dir: Path, plan: runtime.RuntimePlan, layer: Path,
                         verbose: bool) -> None:
    """RUNTIME layer: interpreter + stdlib + .so + /etc.

    Intentionally **independent of app code**: editing `app.py` does not change
    this layer, so it is reused as-is on rebuild (build cache). It only changes
    when the interpreter or binary dependencies change.
    """
    # Binaries to analyze for .so deps: interpreter, native binary, and C
    # extensions from site-packages (we read .so from the HOST source).
    binaries: list[Path] = []
    if plan.interpreter_host:
        _copy_into_rootfs(plan.interpreter_host, layer)
        binaries.append(plan.interpreter_host)
    if plan.runtime == "binary":
        native = app_dir / Path(plan.entrypoint[0]).name
        if native.exists():
            binaries.append(native)
    for src, _ in plan.site_packages:
        binaries.extend(src.rglob("*.so"))

    all_libs: set[Path] = set()
    for b in binaries:
        all_libs |= ldd.shared_libs(b)
    for lib in sorted(all_libs):
        _copy_into_rootfs(lib, layer)
    if verbose:
        print(f"  runtime layer: {len(all_libs)} shared libraries")

    # Runtime directories (e.g. Python stdlib).
    for d in plan.extra_dirs_host:
        dest = layer / str(d).lstrip("/")
        if not dest.exists():
            shutil.copytree(d, dest, symlinks=True,
                            ignore=shutil.ignore_patterns("__pycache__", "*.pyc", "test", "tests"))
            if verbose:
                print(f"  runtime layer: embedded {d}")

    _write_etc(layer)


def _build_app_layer(app_dir: Path, plan: runtime.RuntimePlan, layer: Path,
                     verbose: bool) -> None:
    """APP layer: application code + site-packages. Small and volatile."""
    app_dest = layer / "app"
    shutil.copytree(
        app_dir, app_dest, symlinks=True, dirs_exist_ok=True,
        ignore=shutil.ignore_patterns(
            ".venv", "venv", "site-packages", "__pycache__", "*.pyc", ".git"
        ),
    )
    for src, rootfs_rel in plan.site_packages:
        dest = layer / rootfs_rel.lstrip("/")
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(src, dest, symlinks=True,
                        ignore=shutil.ignore_patterns("__pycache__", "*.pyc"))
        if verbose:
            print(f"  app layer: site-packages from {src}")


def _tar_deterministic(root: Path) -> bytes:
    """Deterministic tar of `root` content (normalized mtime/uid/gid, sorted entries).
    Same content → same bytes → same hash → reusable build cache.
    """
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w", format=tarfile.GNU_FORMAT) as tf:
        entries = sorted(p for p in root.rglob("*"))
        for path in entries:
            arcname = str(path.relative_to(root))
            ti = tf.gettarinfo(str(path), arcname=arcname)
            ti.mtime = 0
            ti.uid = ti.gid = 0
            ti.uname = ti.gname = ""
            if ti.isreg():
                with open(path, "rb") as f:
                    tf.addfile(ti, f)
            else:  # dir, symlink…
                tf.addfile(ti)
    return buf.getvalue()


def _zstd(raw: bytes) -> bytes:
    return subprocess.run(["zstd", "-19", "-T0", "-c"], input=raw,
                          capture_output=True, check=True).stdout


def _build_cache_dir() -> Path:
    base = os.environ.get("XDG_CACHE_HOME")
    d = (Path(base) if base else Path.home() / ".cache") / "xbin" / "build"
    d.mkdir(parents=True, exist_ok=True)
    return d


def _compress_layer_cached(raw_tar: bytes, reuse: bool, verbose: bool,
                           label: str) -> bytes:
    """Compress a layer, reusing the compressed blob from the build cache
    if an identical layer was already compressed (key = tar hash).
    """
    if not reuse:
        comp = _zstd(raw_tar)
        if verbose:
            print(f"  {label}: {len(raw_tar)/1e6:.1f}MB -> {len(comp)/1e6:.1f}MB (zstd)")
        return comp

    key = hashlib.sha256(raw_tar).hexdigest()
    blob = _build_cache_dir() / f"{key}.zst"
    if blob.is_file():
        if verbose:
            print(f"  {label}: reused from build cache (no recompression) ✓")
        return blob.read_bytes()
    comp = _zstd(raw_tar)
    blob.write_bytes(comp)
    if verbose:
        print(f"  {label}: {len(raw_tar)/1e6:.1f}MB -> {len(comp)/1e6:.1f}MB (zstd, cached)")
    return comp


def build(app_path: str, output: str | None, key_path: str | None = None,
          verbose: bool = True) -> str:
    """Build a .xbin (v3 format, multi-layer). Returns the output path.

    Layout: [stub][runtime layer][app layer][metadata][footer].
    When key_path is given, signs inline:
      [stub][payload][metadata][sig_block][v3 footer with FLAG_SIGNED].

    On rebuild, only the app layer is recompressed (runtime is cached).
    """
    app_dir = Path(app_path).resolve()
    if not app_dir.is_dir():
        raise NotADirectoryError(f"{app_dir} is not a directory")

    name = app_dir.name
    out_path = Path(output) if output else Path.cwd() / f"{name}.xbin"

    stub = find_stub()
    plan = runtime.detect(app_dir)
    if verbose:
        print(f"[xbin] building '{name}'")
        print(f"  runtime: {plan.runtime}")
        print(f"  entrypoint: {' '.join(plan.entrypoint)}")

    t0 = time.time()
    with tempfile.TemporaryDirectory(prefix="xbin-build-") as tmp:
        rt_dir = Path(tmp) / "runtime"
        app_dir_layer = Path(tmp) / "app"
        rt_dir.mkdir()
        app_dir_layer.mkdir()

        _build_runtime_layer(app_dir, plan, rt_dir, verbose)
        _build_app_layer(app_dir, plan, app_dir_layer, verbose)

        rt_tar = _tar_deterministic(rt_dir)
        app_tar = _tar_deterministic(app_dir_layer)

    # Runtime layer is stable → reusable from build cache.
    # App layer is small and volatile → always recompressed.
    rt_comp = _compress_layer_cached(rt_tar, reuse=True, verbose=verbose,
                                     label="runtime layer")
    app_comp = _compress_layer_cached(app_tar, reuse=False, verbose=verbose,
                                      label="app layer")

    # Absolute offsets within the final file.
    stub_bytes = stub.read_bytes()
    rt_offset = len(stub_bytes)
    app_offset = rt_offset + len(rt_comp)
    meta_offset = app_offset + len(app_comp)

    layers = [
        {"kind": "runtime", "offset": rt_offset, "csize": len(rt_comp),
         "usize": len(rt_tar), "sha256": hashlib.sha256(rt_comp).hexdigest()},
        {"kind": "app", "offset": app_offset, "csize": len(app_comp),
         "usize": len(app_tar), "sha256": hashlib.sha256(app_comp).hexdigest()},
    ]
    meta = {
        "name": name,
        "xbin_version": XBIN_VERSION,
        "created": datetime.now(timezone.utc).isoformat(),
        "runtime": plan.runtime,
        "isolation": 0,
        "entrypoint": plan.entrypoint,
        "env": plan.env,
        "cwd": plan.cwd,
        "layers": layers,
    }
    meta_bytes = json.dumps(meta, separators=(",", ":")).encode()

    # Integrity: SHA-256 of (layer region + metadata).
    payload_csize = len(rt_comp) + len(app_comp)
    payload = rt_comp + app_comp
    integrity = hashlib.sha256(payload + meta_bytes).digest()

    footer = fmt.Footer(
        format_version=3 if key_path else 2,
        arch=fmt.ARCH_X86_64,
        flags=0,
        payload_offset=rt_offset,
        payload_csize=payload_csize,
        payload_usize=0,  # unused in v2/v3 (sizes are per-layer)
        payload_sha256=integrity,
        meta_offset=meta_offset,
        meta_size=len(meta_bytes),
    )

    with open(out_path, "wb") as f:
        f.write(stub_bytes)
        f.write(payload)
        f.write(meta_bytes)

        if key_path:
            body_hash = hashlib.sha256(payload + meta_bytes).digest()
            sig = crypto.sign(key_path, body_hash)
            sig_block = struct.pack("<I", 64) + sig  # 68 bytes
            sig_offset = f.tell()
            f.write(sig_block)
            footer.format_version = 3
            footer.flags |= fmt.FLAG_SIGNED
            footer.sig_offset = sig_offset

        f.write(footer.pack())
    os.chmod(out_path, 0o755)

    size = out_path.stat().st_size
    if verbose:
        label = "signed" if key_path else "unsigned"
        print(f"[xbin] wrote {out_path} ({size/1e6:.1f}MB, {label}) in {time.time()-t0:.1f}s")
    return str(out_path)
