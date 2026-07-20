"""xbin build: analyze an app, build the rootfs, assemble the .xbin."""

from __future__ import annotations

import hashlib
import io
import json
import os
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
from datetime import UTC, datetime
from pathlib import Path

try:
    import tomllib
except ImportError:
    import tomli as tomllib  # type: ignore[no-redef]

from . import crypto
from . import format as fmt
from ._util import find_binary
from .analyzer import elf, runtime
from .analyzer.dockerfile import detect_from_dockerfile
from .analyzer.fetch import fetch_deps
from .analyzer.lockfile import (
    detect_or_read_lock,
    write_lock_from_results,
)
from .analyzer.python_ast import detect_from_python_source, merge_deps
from .cross import (
    _vendored_python_version,
    cross_python_root,
    host_arch,
    is_cross_build,
    pip_download_target,
    resolve_cross_python,
)
from .encrypt import encrypt_payload

XBIN_VERSION = "0.1.0"

_NO_KEYS_MSG = (
    "no .key files found in ~/.xbin/keys/; use --key or run 'xbin keygen' first"
)


def find_stub(target_arch: str | None = None) -> Path:
    """Locate the compiled launcher stub."""
    return find_binary(
        "xbin-stub",
        "XBIN_STUB",
        "launcher stub not found. Build it first:\n"
        "  cd stub && cargo build --release --target x86_64-unknown-linux-musl\n"
        "  # or for cross-compilation:\n"
        "  cd stub && cargo build --release --target aarch64-unknown-linux-musl",
        target_arch=target_arch,
    )


def find_crypto(target_arch: str | None = None) -> Path:
    """Locate the compiled xbin-crypto binary."""
    return find_binary(
        "xbin-crypto",
        "XBIN_CRYPTO",
        "xbin-crypto not found. Build it first:\n"
        "  cd stub && cargo build --release --target x86_64-unknown-linux-musl",
        target_arch=target_arch,
    )


def _resolve_app_path(app_path: str) -> Path:
    """Resolve an app path that may be relative.

    Tries CWD, then $XBIN_ORIG_CWD (set by the Rust launcher), then walks up
    from the xbin package directory to find the project root.
    """
    resolved = Path(app_path)
    if resolved.is_absolute():
        return resolved.resolve()

    app_dir = (Path.cwd() / app_path).resolve()
    if app_dir.is_dir():
        return app_dir

    orig = os.environ.get("XBIN_ORIG_CWD")
    if orig and (Path(orig) / app_path).resolve().is_dir():
        return (Path(orig) / app_path).resolve()

    # Walk up from the xbin package to find the project root.
    here = Path(__file__).resolve().parent
    for parent in [here, here.parent, here.parent.parent]:
        candidate = parent / app_path
        if candidate.is_dir():
            return candidate.resolve()

    return app_dir  # will fail the is_dir() check downstream


def _copy_into_rootfs(host_path: Path, rootfs: Path) -> None:
    """Copy a host file into the rootfs, preserving its absolute path.

    E.g. /usr/lib/x86_64-linux-gnu/libc.so.6 -> rootfs/usr/lib/x86_64-linux-gnu/libc.so.6
    Resolves symlinks to real targets but re-creates the symlink itself.
    Detects and fixes common broken-symlink patterns such as merged-/usr
    layouts where /lib → usr/lib on the host but not in the rootfs.
    """
    rel = str(host_path).lstrip("/")
    dest = rootfs / rel
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.exists() or dest.is_symlink():
        return
    if host_path.is_symlink():
        target = os.readlink(host_path)
        real = host_path.resolve()
        _copy_into_rootfs(real, rootfs)
        try:
            dest.symlink_to(target)
        except FileExistsError:
            return
        # Check if the symlink resolves correctly inside the rootfs.
        # If not, the real file was placed at a different path (e.g.
        # the host has /lib -> usr/lib but the rootfs doesn't).
        expected = (dest.parent / target).resolve()
        if not expected.exists():
            real_rel = str(real).lstrip("/")
            real_in_rootfs = rootfs / real_rel
            if real_in_rootfs.exists():
                dest.unlink()
                dest.symlink_to(os.path.relpath(real_in_rootfs, dest.parent))
    else:
        shutil.copy2(host_path, dest)


def _write_etc(rootfs: Path) -> None:
    """Minimal /etc for apps that need user/DNS resolution."""
    etc = rootfs / "etc"
    etc.mkdir(parents=True, exist_ok=True)
    (etc / "passwd").write_text("root:x:0:0:root:/root:/bin/sh\n")
    (etc / "group").write_text("root:x:0:\n")
    (etc / "hosts").write_text("127.0.0.1 localhost\n::1 localhost\n")
    (etc / "nsswitch.conf").write_text("hosts: files dns\n")
    (etc / "resolv.conf").write_text("nameserver 1.1.1.1\n")


def _install_cross_python(vendored: Path, rootfs: Path, verbose: bool) -> str:
    """Install vendored cross-compilation Python into rootfs at /opt/cross-python/.

    Returns the vendored Python version string (e.g. '3.12') as detected
    from the lib/ directory structure.

    The vendored Python is self-contained — stdlib, shared libraries, and
    bundled extensions (_ssl, _hashlib, _ctypes, etc.) are all included.
    No external .so resolution is needed.
    """
    dest = rootfs / "opt" / "cross-python"
    shutil.copytree(vendored, dest, symlinks=True)
    if verbose:
        print(f"  cross-python: installed {vendored} -> {dest}", file=sys.stderr)
    return _vendored_python_version(vendored) or "3"


def _build_runtime_layer(
    app_dir: Path,
    plan: runtime.RuntimePlan,
    layer: Path,
    verbose: bool,
    cross_python_root: Path | None = None,
) -> None:
    """RUNTIME layer: interpreter + stdlib + .so + /etc.

    Intentionally **independent of app code**: editing `app.py` does not change
    this layer, so it is reused as-is on rebuild (build cache). It only changes
    when the interpreter or binary dependencies change.

    When cross_python_root is set, uses the vendored cross-compilation Python
    instead of the host's Python. The vendored Python is self-contained and
    does not need .so dependency resolution.
    """
    if cross_python_root:
        _install_cross_python(cross_python_root, layer, verbose)
        _write_etc(layer)
        return

    # Binaries to analyze for .so deps: interpreter, native binary, C
    # extensions from site-packages, and stdlib extensions (e.g. _ssl, _lzma).
    # Stdlib lib-dynload/ .so files are the most common source of version
    # mismatches (e.g. Python bundled OpenSSL vs system OpenSSL).
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
    # Scan stdlib lib-dynload/ for C extensions (_ssl, _lzma, _sqlite3, etc.)
    # that may link against bundled libs (e.g. OpenSSL) via $ORIGIN RPATH.
    for d in plan.extra_dirs_host:
        lib_dynload = d / "lib-dynload"
        if lib_dynload.is_dir():
            binaries.extend(lib_dynload.glob("*.so"))

    all_libs: set[Path] = set()
    for b in binaries:
        all_libs |= elf.shared_libs(b)
    for lib in sorted(all_libs):
        _copy_into_rootfs(lib, layer)
    if verbose:
        print(f"  runtime layer: {len(all_libs)} shared libraries", file=sys.stderr)
        for lib in sorted(all_libs):
            r = str(lib.resolve()) if lib.is_symlink() else ""
            arrow = f" -> {r}" if r and r != str(lib) else ""
            print(f"    {lib}{arrow}", file=sys.stderr)

    # Runtime directories (e.g. Python stdlib).
    # Exclude site-packages (belongs in app layer, not runtime), test dirs,
    # idlelib, ensurepip, turtledemo, pydoc_data, and build config.
    # NOTE: we intentionally INCLUDE __pycache__/ and *.pyc so Python can
    # load pre-compiled bytecode instead of re-compiling on every startup.
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

    _write_etc(layer)


def _build_app_layer(
    app_dir: Path, plan: runtime.RuntimePlan, layer: Path, verbose: bool
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
    for src, rootfs_rel in plan.site_packages:
        dest = layer / rootfs_rel.lstrip("/")
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(src, dest, symlinks=True)
        if verbose:
            print(f"  app layer: {src.name} from {src}", file=sys.stderr)


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
    return subprocess.run(
        ["zstd", "-19", "-T0", "-c"], input=raw, capture_output=True, check=True
    ).stdout


def _mksquashfs(source_dir: Path) -> bytes:
    """Create a squashfs image from a directory. Returns the raw squashfs bytes."""
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


def _build_cache_dir() -> Path:
    base = os.environ.get("XDG_CACHE_HOME")
    d = (Path(base) if base else Path.home() / ".cache") / "xbin" / "build"
    d.mkdir(parents=True, exist_ok=True)
    return d


def _compress_layer_cached(
    raw_tar: bytes, reuse: bool, verbose: bool, label: str
) -> bytes:
    """Compress a layer, reusing the compressed blob from the build cache
    if an identical layer was already compressed (key = tar hash).
    """
    if not reuse:
        comp = _zstd(raw_tar)
        if verbose:
            print(
                f"  {label}: {len(raw_tar)/1e6:.1f}MB -> {len(comp)/1e6:.1f}MB (zstd)",
                file=sys.stderr,
            )
        return comp

    key = hashlib.sha256(raw_tar).hexdigest()
    blob = _build_cache_dir() / f"{key}.zst"
    if blob.is_file():
        if verbose:
            print(f"  {label}: reused from build cache (no recompression) ✓", file=sys.stderr)
        return blob.read_bytes()
    comp = _zstd(raw_tar)
    blob.write_bytes(comp)
    if verbose:
        print(
            f"  {label}: {len(raw_tar)/1e6:.1f}MB -> {len(comp)/1e6:.1f}MB (zstd, cached)",
            file=sys.stderr,
        )
    return comp


def _pip_install_requirements(
    app_dir: Path,
    work_dir: Path,
    plan: runtime.RuntimePlan,
    verbose: bool,
    target_arch: str | None = None,
) -> None:
    """Install pip dependencies for the target architecture.

    When *target_arch* is set (cross-compilation), uses
    ``pip download --only-binary=:all: --platform <tag>`` to fetch
    target-arch wheels.  Pure-Python wheels (py3-none-any) work universally;
    manylinux wheels must match the target arch tag.  Compiled extensions
    that do not provide a matching wheel are rejected by pip.

    When *target_arch* is None (native build), creates a temporary venv
    and pip-installs normally.  This captures any compiled extensions
    available on the host.

    The resulting site-packages directory is appended to plan.site_packages.

    Raises RuntimeError if pip fails.
    """
    req = app_dir / "requirements.txt"
    if not (req.is_file() and req.stat().st_size > 0):
        return

    if target_arch:
        dest = work_dir / "pip-target"
        dest.mkdir(parents=True, exist_ok=True)
        pip_download_target(req, dest, target_arch, verbose)
        # Unpack downloaded wheels into site-packages
        sp_dir = dest / "site-packages"
        sp_dir.mkdir(parents=True, exist_ok=True)
        for whl in dest.glob("*.whl"):
            _unpack_wheel(whl, sp_dir, verbose)
        # Remove the .whl files, keep only extracted site-packages
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


def _unpack_wheel(whl: Path, dest: Path, verbose: bool) -> None:
    """Extract a .whl file (which is a ZIP archive) into *dest*."""
    import zipfile

    name = whl.name
    try:
        with zipfile.ZipFile(whl, "r") as zf:
            for member in zf.namelist():
                # Skip dist-info, RECORD, etc. — pure code only
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


def _copy_binary_into_rootfs(bin_path: Path, rootfs: Path, verbose: bool) -> None:
    """Copy a binary and its shared libraries into rootfs."""
    real = bin_path.resolve()
    _copy_into_rootfs(real, rootfs)
    if verbose:
        print(f"    binary: {real}", file=sys.stderr)
    for lib in elf.shared_libs(real):
        _copy_into_rootfs(lib, rootfs)


def _resolve_service_binary(bin_name: str) -> Path | None:
    """Find a service binary on the host, trying absolute and PATH lookup."""
    if bin_name.startswith("/"):
        bp = Path(bin_name)
        if not bp.exists():
            for candidate in [bp, Path(f"/usr{bin_name}")]:
                if candidate.exists():
                    return candidate
        return bp if bp.exists() else None
    return Path(shutil.which(bin_name) or f"/usr/{bin_name}")


def _collect_service_bins(services: list[dict], verbose: bool) -> set[Path]:
    """Resolve all service binaries and copy their shared libs into rootfs."""
    bins: set[Path] = set()
    for svc in services:
        bp = _resolve_service_binary(svc["cmd"][0])
        if bp and bp.exists():
            bins.add(bp)
            if verbose:
                print(f"  service '{svc['name']}': {bp}", file=sys.stderr)
        else:
            print(f"  WARNING: binary not found for '{svc['name']}': {svc['cmd'][0]}", file=sys.stderr)
    return bins


def _read_signing_seed(key_path: str) -> bytes:
    """Read a 32-byte Ed25519 signing seed from a .key file."""
    seed = Path(key_path).read_bytes()
    if len(seed) != 32:
        raise ValueError(f"signing key must be 32 bytes, got {len(seed)}")
    return seed


def _build_meta_json(
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
    if payload_format:
        meta["payload_format"] = payload_format
    if seccomp:
        meta["seccomp"] = True
    if crypto:
        meta["crypto"] = crypto
    if services:
        meta["services"] = services
    return json.dumps(meta, separators=(",", ":")).encode()


def _assemble_xbin(
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
            _sign_and_write(f, footer, key_path, payload, meta_bytes)
        else:
            f.write(footer.pack())
    os.chmod(out_path, 0o755)
    return out_path.stat().st_size


def _build_manifest(
    app_dir: Path,
    manifest: dict,
    output: str | None,
    key_path: str | None,
    verbose: bool,
) -> str:
    """Build a multi-service .xbin from xbin.toml manifest."""
    name = manifest.get("app", {}).get("name", app_dir.name)
    isolation = manifest.get("app", {}).get("isolation", 0)
    seccomp = manifest.get("app", {}).get("seccomp", False)
    services = manifest.get("services", [])
    if not services:
        raise ValueError("xbin.toml has no [[services]]")

    out_path = Path(output) if output else Path.cwd() / f"{name}.xbin"
    stub = find_stub()
    t0 = time.time()

    with tempfile.TemporaryDirectory(prefix="xbin-build-") as tmp:
        tmp_path = Path(tmp)
        rt_dir = tmp_path / "runtime"
        app_dir_layer = tmp_path / "app"
        rt_dir.mkdir()
        app_dir_layer.mkdir()

        all_bins = _collect_service_bins(services, verbose)
        _copy_service_layers(all_bins, rt_dir, verbose)
        _write_etc(rt_dir)

        _copy_app_files(app_dir, app_dir_layer)
        _install_manifest_pip(app_dir, services, tmp_path, app_dir_layer, verbose)
        (rt_dir / "data" / "db").mkdir(parents=True, exist_ok=True)
        (rt_dir / "tmp").mkdir(parents=True, exist_ok=True)

        rt_tar = _tar_deterministic(rt_dir)
        app_tar = _tar_deterministic(app_dir_layer)

    rt_comp = _compress_layer_cached(
        rt_tar, reuse=False, verbose=verbose, label="runtime layer"
    )
    app_comp = _compress_layer_cached(
        app_tar, reuse=False, verbose=verbose, label="app layer"
    )

    layers = [
        {
            "kind": "runtime",
            "offset": len(stub.read_bytes()),
            "csize": len(rt_comp),
            "usize": len(rt_tar),
            "sha256": hashlib.sha256(rt_comp).hexdigest(),
        },
        {
            "kind": "app",
            "offset": len(stub.read_bytes()) + len(rt_comp),
            "csize": len(app_comp),
            "usize": len(app_tar),
            "sha256": hashlib.sha256(app_comp).hexdigest(),
        },
    ]
    meta_services = _build_service_metadata(services, all_bins)
    meta_bytes = _build_meta_json(
        name=name,
        runtime="multi",
        isolation=isolation,
        entrypoint=[],
        env={},
        layers=layers,
        services=meta_services,
        seccomp=seccomp,
    )
    payload = rt_comp + app_comp
    size = _assemble_xbin(out_path, stub, payload, meta_bytes, key_path)
    label = "signed" if key_path else "unsigned"
    print(
        f"[xbin] wrote {out_path} ({size/1e6:.1f}MB, {label}) in {time.time()-t0:.1f}s",
        file=sys.stderr,
    )
    return str(out_path)


def _copy_service_layers(all_bins: set[Path], rt_dir: Path, verbose: bool) -> None:
    """Copy service binaries and their shared libraries into the runtime dir."""
    all_libs: set[Path] = set()
    for b in all_bins:
        all_libs |= elf.shared_libs(b)
    for lib in sorted(all_libs):
        _copy_into_rootfs(lib, rt_dir)
    for b in sorted(all_bins):
        _copy_into_rootfs(b, rt_dir)
    if verbose:
        print(
            f"  runtime layer: {len(all_bins)} binaries, {len(all_libs)} shared libraries",
            file=sys.stderr,
        )


def _copy_app_files(app_dir: Path, app_dir_layer: Path) -> None:
    """Copy application files into the app layer directory."""
    app_dest = app_dir_layer / "app"
    shutil.copytree(
        app_dir,
        app_dest,
        symlinks=True,
        dirs_exist_ok=True,
        ignore=shutil.ignore_patterns(
            ".venv",
            "venv",
            "site-packages",
            "node_modules",
            ".git",
            "xbin.toml",
            "__pycache__",
        ),
    )


def _install_manifest_pip(
    app_dir: Path,
    services: list[dict],
    tmp_path: Path,
    app_dir_layer: Path,
    verbose: bool,
) -> None:
    """Install pip requirements for Python services in manifest mode."""
    req = app_dir / "requirements.txt"
    if not (req.is_file() and req.stat().st_size > 0):
        return
    for svc in services:
        if svc["cmd"][0] in (
            "python3",
            "python",
            "/usr/bin/python3",
            "/usr/bin/python",
        ):
            venv_dir = tmp_path / ".xbin-venv"
            _pip_install_requirements(
                app_dir, tmp_path, _ManifestPlan(svc, app_dir), verbose
            )
            py_ver = f"python{sys.version_info.major}.{sys.version_info.minor}"
            sp_src = venv_dir / "lib" / py_ver / "site-packages"
            sp_dest = app_dir_layer / "app" / "site-packages"
            if sp_src.is_dir():
                shutil.copytree(sp_src, sp_dest, symlinks=True, dirs_exist_ok=True)
            break


def _build_service_metadata(services: list[dict], all_bins: set[Path]) -> list[dict]:
    """Build the services array for metadata JSON, resolving cmd[0] to rootfs paths."""
    result = []
    for svc in services:
        cmd = list(svc["cmd"])
        bin_name = cmd[0]
        if bin_name.startswith("/"):
            real = Path(bin_name).resolve()
            for b in sorted(all_bins):
                if b.resolve() == real or b == Path(bin_name):
                    cmd[0] = f"/{str(b).lstrip('/')}"
                    break
        meta_svc: dict = {"name": svc["name"], "cmd": cmd}
        if "env" in svc:
            meta_svc["env"] = svc["env"]
        if svc.get("ready_port"):
            meta_svc["ready_port"] = svc["ready_port"]
        if svc.get("ready_timeout"):
            meta_svc["ready_timeout"] = svc["ready_timeout"]
        result.append(meta_svc)
    return result


class _ManifestPlan:
    """Minimal shim so _pip_install_requirements works for manifest builds."""

    def __init__(self, svc: dict, app_dir: Path):
        self.runtime = "python"
        self.entrypoint = svc["cmd"]
        self.env: dict[str, str] = svc.get("env", {})
        self.cwd = "/app"
        self.site_packages: list[tuple[Path, str]] = []


def _sign_and_write(
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


def build(
    app_path: str,
    output: str | None,
    key_path: str | None = None,
    isolation: int = 0,
    seccomp: bool = False,
    encrypt: bool = False,
    squashfs: bool = False,
    verbose: bool = True,
    redetect: bool = False,
    target: str | None = None,
) -> str:
    """Build a .xbin (v3/v4/v5 format, multi-layer). Returns the output path.

    Layout: [stub][runtime layer][app layer][metadata][footer].
    When key_path is given, signs inline:
      [stub][payload][metadata][sig_block][v3 footer with FLAG_SIGNED].
    When encrypt is True (requires key_path), encrypts payload with AES-256-GCM
    and bumps to v4 footer (crypto_suite in payload_usize).
    When squashfs is True, layers are squashfs images (v5 footer, payload_format
    in metadata).
    When target is set, cross-compiles for the specified architecture using a
    vendored python-build-standalone Python interpreter. Only pure-Python apps
    (no compiled extensions) are supported in cross-build mode.
    """
    app_dir = _resolve_app_path(app_path)
    if not app_dir.is_dir():
        raise NotADirectoryError(f"{app_dir} is not a directory")

    manifest_path = app_dir / "xbin.toml"
    if manifest_path.is_file():
        with open(manifest_path, "rb") as f:
            manifest = tomllib.load(f)
        if verbose:
            print(f"[xbin] building '{app_dir.name}' (manifest mode)", file=sys.stderr)
        return _build_manifest(app_dir, manifest, output, key_path, verbose)

    # --- Dependency detection (Features A/B/C) + lockfile ---
    locked_deps = detect_or_read_lock(app_dir, redetect=redetect, verbose=verbose)
    if locked_deps is not None:
        # Lock is fresh — use locked deps, skip detection.
        dep_list = locked_deps
    else:
        # No lock or stale — run full detection pipeline.
        dockerfile_deps = detect_from_dockerfile(app_dir)
        ast_deps = detect_from_python_source(app_dir)
        dep_list = merge_deps(dockerfile_deps, ast_deps)
        if dep_list:
            _, results = fetch_deps(dep_list, verbose=verbose)
            write_lock_from_results(app_dir, dep_list, results, verbose=verbose)
        elif verbose:
            print("[xbin] no external dependencies detected", file=sys.stderr)

    name = app_dir.name
    out_path = Path(output) if output else Path.cwd() / f"{name}.xbin"
    stub = find_stub(target_arch=target)
    plan = runtime.detect(app_dir)
    if verbose:
        print(f"[xbin] building '{name}'", file=sys.stderr)
        print(f"  runtime: {plan.runtime}", file=sys.stderr)
        print(f"  entrypoint: {' '.join(plan.entrypoint)}", file=sys.stderr)

    # --- Cross-compilation setup ---
    cross_root: Path | None = None
    if target and is_cross_build(target):
        if plan.runtime != "python":
            raise ValueError(
                f"cross-build for {target} requires a Python runtime, "
                f"got '{plan.runtime}'"
            )
        if verbose:
            print(f"  target: {target} (cross-compilation)", file=sys.stderr)
        vendored = resolve_cross_python(target)
        cross_root = cross_python_root(vendored)
        plan.entrypoint[0] = "/opt/cross-python/bin/python3"
    elif target:
        if verbose:
            print(f"  target: {target} (native build)", file=sys.stderr)
    else:
        if verbose:
            print(f"  target: {host_arch()} (native build)", file=sys.stderr)

    t0 = time.time()
    if squashfs:
        rt_sqfs, app_sqfs = _build_layers_squashfs(
            app_dir, plan, verbose, cross_python_root=cross_root,
            target_arch=target,
        )
        stub_bytes = stub.read_bytes()
        rt_offset = len(stub_bytes)
        payload = rt_sqfs + app_sqfs
        layers = [
            {
                "kind": "squashfs",
                "offset": rt_offset,
                "csize": len(rt_sqfs),
                "usize": len(rt_sqfs),
                "sha256": hashlib.sha256(rt_sqfs).hexdigest(),
            },
            {
                "kind": "squashfs",
                "offset": rt_offset + len(rt_sqfs),
                "csize": len(app_sqfs),
                "usize": len(app_sqfs),
                "sha256": hashlib.sha256(app_sqfs).hexdigest(),
            },
        ]
        payload_format = fmt.PAYLOAD_FORMAT_SQUASHFS
    else:
        rt_comp, app_comp, rt_tar, app_tar = _build_layers(
            app_dir,
            plan,
            verbose,
            cross_python_root=cross_root,
            target_arch=target,
        )
        stub_bytes = stub.read_bytes()
        rt_offset = len(stub_bytes)
        payload = rt_comp + app_comp
        layers = [
            {
                "kind": "runtime",
                "offset": rt_offset,
                "csize": len(rt_comp),
                "usize": len(rt_tar),
                "sha256": hashlib.sha256(rt_comp).hexdigest(),
            },
            {
                "kind": "app",
                "offset": rt_offset + len(rt_comp),
                "csize": len(app_comp),
                "usize": len(app_tar),
                "sha256": hashlib.sha256(app_comp).hexdigest(),
            },
        ]
        payload_format = ""

    # --- Encryption (AES-256-GCM, requires --key for signing seed) ---
    crypto_meta: dict = {}
    if encrypt:
        if not key_path:
            raise ValueError(
                "--encrypt requires --key (signing seed used for AES key derivation)"
            )
        signing_seed = _read_signing_seed(key_path)
        pre_enc_size = len(payload)
        payload, enc_meta = encrypt_payload(payload, signing_seed)
        crypto_meta = enc_meta
        if verbose:
            print(
                f"  encrypted: {pre_enc_size/1e6:.1f}MB -> {len(payload)/1e6:.1f}MB (AES-256-GCM)",
                file=sys.stderr,
            )

    meta_bytes = _build_meta_json(
        name=name,
        runtime=plan.runtime,
        isolation=isolation,
        entrypoint=plan.entrypoint,
        env=plan.env,
        layers=layers,
        seccomp=seccomp,
        crypto=crypto_meta if crypto_meta else None,
        payload_format=payload_format,
    )
    size = _assemble_xbin(
        out_path,
        stub,
        payload,
        meta_bytes,
        key_path,
        encrypt=encrypt,
        squashfs=squashfs,
        target_arch=target,
    )
    if verbose:
        label = "signed" if key_path else "unsigned"
        enc_label = "+encrypted" if encrypt else ""
        sqfs_label = "+squashfs" if squashfs else ""
        print(
            f"[xbin] wrote {out_path} ({size/1e6:.1f}MB, {label}{enc_label}{sqfs_label}) in {time.time()-t0:.1f}s",
            file=sys.stderr,
        )
    return str(out_path)


def _build_layers(
    app_dir: Path,
    plan: runtime.RuntimePlan,
    verbose: bool,
    cross_python_root: Path | None = None,
    target_arch: str | None = None,
) -> tuple[bytes, bytes, bytes, bytes]:
    """Build runtime and app layers, returning (rt_comp, app_comp, rt_tar, app_tar)."""
    req = app_dir / "requirements.txt"
    with tempfile.TemporaryDirectory(prefix="xbin-build-") as tmp:
        tmp_path = Path(tmp)
        if (
            plan.runtime == "python"
            and req.is_file()
            and req.stat().st_size > 0
            and not plan.site_packages
        ):
            _pip_install_requirements(
                app_dir, tmp_path, plan, verbose, target_arch=target_arch
            )

        rt_dir = tmp_path / "runtime"
        app_dir_layer = tmp_path / "app"
        rt_dir.mkdir()
        app_dir_layer.mkdir()

        _build_runtime_layer(
            app_dir, plan, rt_dir, verbose, cross_python_root=cross_python_root
        )
        _build_app_layer(app_dir, plan, app_dir_layer, verbose)

        rt_tar = _tar_deterministic(rt_dir)
        app_tar = _tar_deterministic(app_dir_layer)

    # Runtime layer is stable → reusable from build cache.
    # App layer is small and volatile → always recompressed.
    rt_comp = _compress_layer_cached(
        rt_tar, reuse=True, verbose=verbose, label="runtime layer"
    )
    app_comp = _compress_layer_cached(
        app_tar, reuse=False, verbose=verbose, label="app layer"
    )
    return rt_comp, app_comp, rt_tar, app_tar


def _build_layers_squashfs(
    app_dir: Path,
    plan: runtime.RuntimePlan,
    verbose: bool,
    cross_python_root: Path | None = None,
    target_arch: str | None = None,
) -> tuple[bytes, bytes]:
    """Build runtime and app layers as squashfs images.

    Returns (rt_sqfs, app_sqfs) — raw squashfs image bytes.
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
            _pip_install_requirements(
                app_dir, tmp_path, plan, verbose, target_arch=target_arch
            )

        rt_dir = tmp_path / "runtime"
        app_dir_layer = tmp_path / "app"
        rt_dir.mkdir()
        app_dir_layer.mkdir()

        _build_runtime_layer(
            app_dir, plan, rt_dir, verbose, cross_python_root=cross_python_root
        )
        _build_app_layer(app_dir, plan, app_dir_layer, verbose)

        rt_sqfs = _mksquashfs(rt_dir)
        app_sqfs = _mksquashfs(app_dir_layer)

    if verbose:
        print(f"  runtime layer: {len(rt_sqfs)/1e6:.1f}MB (squashfs)", file=sys.stderr)
        print(f"  app layer: {len(app_sqfs)/1e6:.1f}MB (squashfs)", file=sys.stderr)
    return rt_sqfs, app_sqfs
