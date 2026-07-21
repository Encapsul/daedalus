"""Layer construction: runtime layer, app layer, compression, pip install.

Extracted from build.py to keep each file under 300 lines.
"""

from __future__ import annotations

import hashlib
import io
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path

from . import analyzer
from .cross import _vendored_python_version, pip_download_target

# ---------------------------------------------------------------------------
# Rootfs helpers
# ---------------------------------------------------------------------------


def copy_into_rootfs(host_path: Path, rootfs: Path) -> None:
    """Copy a host file into the rootfs, preserving its absolute path.

    E.g. /usr/lib/.../libc.so.6 -> rootfs/usr/lib/.../libc.so.6
    Resolves symlinks to real targets but re-creates the symlink itself.
    Detects and fixes common broken-symlink patterns such as merged-/usr
    layouts where /lib -> usr/lib on the host but not in the rootfs.
    """
    rel = str(host_path).lstrip("/")
    dest = rootfs / rel
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.exists() or dest.is_symlink():
        return
    if host_path.is_symlink():
        target = os.readlink(host_path)
        real = host_path.resolve()
        copy_into_rootfs(real, rootfs)
        try:
            dest.symlink_to(target)
        except FileExistsError:
            return
        expected = (dest.parent / target).resolve()
        if not expected.exists():
            real_rel = str(real).lstrip("/")
            real_in_rootfs = rootfs / real_rel
            if real_in_rootfs.exists():
                dest.unlink()
                dest.symlink_to(os.path.relpath(real_in_rootfs, dest.parent))
    else:
        shutil.copy2(host_path, dest)


def write_etc(rootfs: Path) -> None:
    """Minimal /etc for apps that need user/DNS resolution."""
    etc = rootfs / "etc"
    etc.mkdir(parents=True, exist_ok=True)
    (etc / "passwd").write_text("root:x:0:0:root:/root:/bin/sh\n")
    (etc / "group").write_text("root:x:0:\n")
    (etc / "hosts").write_text("127.0.0.1 localhost\n::1 localhost\n")
    (etc / "nsswitch.conf").write_text("hosts: files dns\n")
    (etc / "resolv.conf").write_text("nameserver 1.1.1.1\n")


# ---------------------------------------------------------------------------
# Cross-compilation Python
# ---------------------------------------------------------------------------


def install_cross_python(vendored: Path, rootfs: Path, verbose: bool) -> str:
    """Install vendored cross-compilation Python into rootfs at /opt/cross-python/.

    Returns the vendored Python version string (e.g. '3.12').
    """
    dest = rootfs / "opt" / "cross-python"
    shutil.copytree(vendored, dest, symlinks=True)
    if verbose:
        print(f"  cross-python: installed {vendored} -> {dest}", file=sys.stderr)
    return _vendored_python_version(vendored) or "3"


# ---------------------------------------------------------------------------
# Runtime layer
# ---------------------------------------------------------------------------


def build_runtime_layer(
    app_dir: Path,
    plan: analyzer.runtime.RuntimePlan,
    layer: Path,
    verbose: bool,
    cross_python_root: Path | None = None,
) -> None:
    """RUNTIME layer: interpreter + stdlib + .so + /etc.

    Independent of app code — reused as-is on rebuild.
    """
    if cross_python_root:
        install_cross_python(cross_python_root, layer, verbose)
        write_etc(layer)
        return

    binaries: list[Path] = []
    if plan.interpreter_host:
        copy_into_rootfs(plan.interpreter_host, layer)
        binaries.append(plan.interpreter_host)
    if plan.runtime == "binary":
        native = app_dir / Path(plan.entrypoint[0]).name
        if native.exists():
            binaries.append(native)
    for src, _ in plan.site_packages:
        binaries.extend(src.rglob("*.so"))
    for d in plan.extra_dirs_host:
        lib_dynload = d / "lib-dynload"
        if lib_dynload.is_dir():
            binaries.extend(lib_dynload.glob("*.so"))

    all_libs: set[Path] = set()
    for b in binaries:
        all_libs |= analyzer.elf.shared_libs(b)
    for lib in sorted(all_libs):
        copy_into_rootfs(lib, layer)
    if verbose:
        print(f"  runtime layer: {len(all_libs)} shared libraries", file=sys.stderr)
        for lib in sorted(all_libs):
            r = str(lib.resolve()) if lib.is_symlink() else ""
            arrow = f" -> {r}" if r and r != str(lib) else ""
            print(f"    {lib}{arrow}", file=sys.stderr)

    _RT_IGNORE = shutil.ignore_patterns(
        "test",
        "tests",
        "site-packages",
        "idlelib",
        "ensurepip",
        "turtledemo",
        "pydoc_data",
        "config-*",
    )
    for d in plan.extra_dirs_host:
        dest = layer / str(d).lstrip("/")
        shutil.copytree(d, dest, symlinks=True, dirs_exist_ok=True, ignore=_RT_IGNORE)
        if verbose:
            print(f"  runtime layer: embedded {d}", file=sys.stderr)

    write_etc(layer)


# ---------------------------------------------------------------------------
# App layer
# ---------------------------------------------------------------------------


def build_app_layer(
    app_dir: Path,
    plan: analyzer.runtime.RuntimePlan,
    layer: Path,
    verbose: bool,
    env_file_path: Path | None = None,
    include_paths: list[Path] | None = None,
) -> None:
    """APP layer: application code + site-packages. Small and volatile."""
    app_dest = layer / "app"
    shutil.copytree(
        app_dir,
        app_dest,
        symlinks=True,
        dirs_exist_ok=True,
        ignore=shutil.ignore_patterns(
            ".venv", "venv", "site-packages", "node_modules", ".git"
        ),
    )
    # Copy external .env file into app layer as .env if it's outside app_dir.
    if (
        env_file_path is not None
        and env_file_path.is_file()
        and not str(env_file_path.resolve()).startswith(str(app_dir.resolve()))
    ):
        dest = app_dest / ".env"
        shutil.copy2(env_file_path, dest)
        if verbose:
            print(f"  app layer: copied {env_file_path.name} -> .env", file=sys.stderr)
    # Copy --include files/dirs into app layer.
    if include_paths:
        for inc_path in include_paths:
            if inc_path.is_dir():
                dest = app_dest / inc_path.name
                if dest.exists():
                    shutil.rmtree(dest)
                shutil.copytree(inc_path, dest, symlinks=True)
            else:
                dest = app_dest / inc_path.name
                shutil.copy2(inc_path, dest)
            if verbose:
                print(f"  app layer: included {inc_path.name}", file=sys.stderr)
    for src, rootfs_rel in plan.site_packages:
        dest = layer / rootfs_rel.lstrip("/")
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(src, dest, symlinks=True)
        if verbose:
            print(f"  app layer: {src.name} from {src}", file=sys.stderr)


# ---------------------------------------------------------------------------
# Compression
# ---------------------------------------------------------------------------


def tar_deterministic(root: Path) -> bytes:
    """Deterministic tar of `root` content (normalized mtime/uid/gid, sorted entries).

    Uses xbin_core (Rust) when available, falls back to Python tarfile.
    """
    try:
        import xbin_core

        return xbin_core.py_create_tar(str(root))
    except ImportError:
        pass
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
            else:
                tf.addfile(ti)
    return buf.getvalue()


def zstd(raw: bytes) -> bytes:
    """Compress bytes with zstd (level 19, multi-threaded).

    Uses xbin_core (Rust) when available, falls back to zstd CLI.
    """
    try:
        import xbin_core

        return xbin_core.py_compress(raw, 19)
    except ImportError:
        pass
    return subprocess.run(
        ["zstd", "-19", "-T0", "-c"], input=raw, capture_output=True, check=True
    ).stdout


def mksquashfs(source_dir: Path) -> bytes:
    """Create a squashfs image from a directory."""
    with tempfile.NamedTemporaryFile(suffix=".squashfs", delete=False) as tmp:
        tmp_path = Path(tmp.name)
    try:
        proc = subprocess.run(
            [
                "mksquashfs",
                str(source_dir),
                str(tmp_path),
                "-comp",
                "zstd",
                "-b",
                "1M",
                "-no-xattrs",
                "-noappend",
                "-no-progress",
                "-quiet",
                "-force-gid",
                "0",
                "-force-uid",
                "0",
            ],
            capture_output=True,
        )
        if proc.returncode != 0:
            raise RuntimeError(
                f"mksquashfs failed (exit {proc.returncode}): "
                f"{proc.stderr.decode(errors='replace').strip()}"
            )
        return tmp_path.read_bytes()
    finally:
        tmp_path.unlink(missing_ok=True)


def build_cache_dir() -> Path:
    base = os.environ.get("XDG_CACHE_HOME")
    d = (Path(base) if base else Path.home() / ".cache") / "xbin" / "build"
    d.mkdir(parents=True, exist_ok=True)
    return d


def compress_layer_cached(
    raw_tar: bytes, reuse: bool, verbose: bool, label: str
) -> bytes:
    """Compress a layer, reusing from build cache if identical tar was already compressed."""
    if not reuse:
        comp = zstd(raw_tar)
        if verbose:
            print(
                f"  {label}: {len(raw_tar)/1e6:.1f}MB -> {len(comp)/1e6:.1f}MB (zstd)",
                file=sys.stderr,
            )
        return comp

    key = hashlib.sha256(raw_tar).hexdigest()
    blob = build_cache_dir() / f"{key}.zst"
    if blob.is_file():
        if verbose:
            print(
                f"  {label}: reused from build cache (no recompression) ✓",
                file=sys.stderr,
            )
        return blob.read_bytes()
    comp = zstd(raw_tar)
    blob.write_bytes(comp)
    if verbose:
        print(
            f"  {label}: {len(raw_tar)/1e6:.1f}MB -> {len(comp)/1e6:.1f}MB (zstd, cached)",
            file=sys.stderr,
        )
    return comp


# ---------------------------------------------------------------------------
# Pip install (Python-specific)
# ---------------------------------------------------------------------------


def pip_install_requirements(
    app_dir: Path,
    work_dir: Path,
    plan: analyzer.runtime.RuntimePlan,
    verbose: bool,
    target_arch: str | None = None,
) -> None:
    """Install pip dependencies for the target architecture.

    Cross-build: pip download --only-binary for target arch.
    Native: temp venv + pip install.
    """
    req = app_dir / "requirements.txt"
    if not (req.is_file() and req.stat().st_size > 0):
        return

    if target_arch:
        dest = work_dir / "pip-target"
        dest.mkdir(parents=True, exist_ok=True)
        pip_download_target(req, dest, target_arch, verbose)
        sp_dir = dest / "site-packages"
        sp_dir.mkdir(parents=True, exist_ok=True)
        for whl in dest.glob("*.whl"):
            unpack_wheel(whl, sp_dir, verbose)
        for whl in dest.glob("*.whl"):
            whl.unlink()
        plan.site_packages.append((sp_dir, "/app/site-packages"))
        plan.env["PYTHONPATH"] = "${ROOTFS}/app/site-packages"
        return

    venv_dir = work_dir / ".xbin-venv"
    subprocess.run(
        [sys.executable, "-m", "venv", str(venv_dir)], check=True, capture_output=True
    )
    pip = str(venv_dir / "bin" / "pip")
    result = subprocess.run(
        [pip, "install", "-r", str(req), "--quiet"], capture_output=True
    )
    if result.returncode != 0:
        stderr = result.stderr.decode(errors="replace").strip()
        raise RuntimeError(f"pip install failed (exit {result.returncode}): {stderr}")
    py_ver = f"python{sys.version_info.major}.{sys.version_info.minor}"
    sp = venv_dir / "lib" / py_ver / "site-packages"
    if not sp.is_dir():
        raise RuntimeError(f"pip-installed site-packages not found at {sp}")
    plan.site_packages.append((sp, "/app/site-packages"))
    plan.env["PYTHONPATH"] = "${ROOTFS}/app/site-packages"
    if verbose:
        print(f"  pip install: {req} -> {sp}", file=sys.stderr)


def unpack_wheel(whl: Path, dest: Path, verbose: bool) -> None:
    """Extract a .whl file (ZIP archive) into *dest*."""
    import zipfile

    name = whl.name
    try:
        with zipfile.ZipFile(whl, "r") as zf:
            for member in zf.namelist():
                if ".dist-info" in member or member.endswith(".dist-info"):
                    continue
                target = dest / member
                target.parent.mkdir(parents=True, exist_ok=True)
                if not member.endswith("/"):
                    target.write_bytes(zf.read(member))
        if verbose:
            print(f"  unpacked wheel: {name}", file=sys.stderr)
    except zipfile.BadZipFile as e:
        if verbose:
            print(f"  warning: bad wheel {name}: {e}", file=sys.stderr)
    except (OSError, RuntimeError) as e:
        if verbose:
            print(f"  warning: failed to unpack {name}: {e}", file=sys.stderr)


# ---------------------------------------------------------------------------
# Consolidated layer builders (replaces _build_layers + _build_layers_squashfs)
# ---------------------------------------------------------------------------


def build_layers(
    app_dir: Path,
    plan: analyzer.runtime.RuntimePlan,
    verbose: bool,
    *,
    squashfs: bool = False,
    cross_python_root: Path | None = None,
    target_arch: str | None = None,
    env_file_path: Path | None = None,
    include_paths: list[Path] | None = None,
) -> tuple[bytes, ...]:
    """Build runtime and app layers.

    Returns:
        if squashfs: (rt_sqfs, app_sqfs)
        else:        (rt_comp, app_comp, rt_tar, app_tar)
    """
    req = app_dir / "requirements.txt"
    with tempfile.TemporaryDirectory(prefix="xbin-build-") as tmp:
        tmp_path = Path(tmp)
        if (
            plan.runtime == "python"
            and req.is_file()
            and req.stat().st_size > 0
            and not plan.site_packages
        ):
            pip_install_requirements(
                app_dir, tmp_path, plan, verbose, target_arch=target_arch
            )

        rt_dir = tmp_path / "runtime"
        app_dir_layer = tmp_path / "app"
        rt_dir.mkdir()
        app_dir_layer.mkdir()

        build_runtime_layer(
            app_dir, plan, rt_dir, verbose, cross_python_root=cross_python_root
        )
        build_app_layer(
            app_dir,
            plan,
            app_dir_layer,
            verbose,
            env_file_path=env_file_path,
            include_paths=include_paths,
        )

        if squashfs:
            rt_sqfs = mksquashfs(rt_dir)
            app_sqfs = mksquashfs(app_dir_layer)
            if verbose:
                print(
                    f"  runtime layer: {len(rt_sqfs)/1e6:.1f}MB (squashfs)",
                    file=sys.stderr,
                )
                print(
                    f"  app layer: {len(app_sqfs)/1e6:.1f}MB (squashfs)",
                    file=sys.stderr,
                )
            return rt_sqfs, app_sqfs

        rt_tar = tar_deterministic(rt_dir)
        app_tar = tar_deterministic(app_dir_layer)

    rt_comp = compress_layer_cached(
        rt_tar, reuse=True, verbose=verbose, label="runtime layer"
    )
    app_comp = compress_layer_cached(
        app_tar, reuse=False, verbose=verbose, label="app layer"
    )
    return rt_comp, app_comp, rt_tar, app_tar
