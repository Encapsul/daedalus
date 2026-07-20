"""xbin manifest: multi-service manifest builds.

Extracted from build.py to keep each file under 300 lines.
"""

from __future__ import annotations

import hashlib
import shutil
import sys
import tempfile
from pathlib import Path

from . import analyzer
from .assembly import assemble_xbin, build_meta_json
from .layers import (
    compress_layer_cached,
    copy_into_rootfs,
    pip_install_requirements,
    tar_deterministic,
    write_etc,
)


class ManifestPlan:
    """Minimal shim so pip_install_requirements works for manifest builds."""

    def __init__(self, svc: dict, app_dir: Path):
        self.runtime = "python"
        self.entrypoint = svc["cmd"]
        self.env: dict[str, str] = svc.get("env", {})
        self.cwd = "/app"
        self.site_packages: list[tuple[Path, str]] = []


def resolve_service_binary(bin_name: str) -> Path | None:
    """Find a service binary on the host, trying absolute and PATH lookup."""
    if bin_name.startswith("/"):
        bp = Path(bin_name)
        if not bp.exists():
            for candidate in [bp, Path(f"/usr{bin_name}")]:
                if candidate.exists():
                    return candidate
        return bp if bp.exists() else None
    return Path(shutil.which(bin_name) or f"/usr/{bin_name}")


def collect_service_bins(services: list[dict], verbose: bool) -> set[Path]:
    """Resolve all service binaries and copy their shared libs into rootfs."""
    bins: set[Path] = set()
    for svc in services:
        bp = resolve_service_binary(svc["cmd"][0])
        if bp and bp.exists():
            bins.add(bp)
            if verbose:
                print(f"  service '{svc['name']}': {bp}", file=sys.stderr)
        else:
            print(
                f"  WARNING: binary not found for '{svc['name']}': {svc['cmd'][0]}",
                file=sys.stderr,
            )
    return bins


def copy_service_layers(all_bins: set[Path], rt_dir: Path, verbose: bool) -> None:
    """Copy service binaries and their shared libraries into the runtime dir."""
    all_libs: set[Path] = set()
    for b in all_bins:
        all_libs |= analyzer.elf.shared_libs(b)
    for lib in sorted(all_libs):
        copy_into_rootfs(lib, rt_dir)
    for b in sorted(all_bins):
        copy_into_rootfs(b, rt_dir)
    if verbose:
        print(
            f"  runtime layer: {len(all_bins)} binaries, {len(all_libs)} shared libraries",
            file=sys.stderr,
        )


def copy_app_files(app_dir: Path, app_dir_layer: Path) -> None:
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


def install_manifest_pip(
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
            pip_install_requirements(
                app_dir, tmp_path, ManifestPlan(svc, app_dir), verbose
            )
            py_ver = f"python{sys.version_info.major}.{sys.version_info.minor}"
            sp_src = venv_dir / "lib" / py_ver / "site-packages"
            sp_dest = app_dir_layer / "app" / "site-packages"
            if sp_src.is_dir():
                shutil.copytree(sp_src, sp_dest, symlinks=True, dirs_exist_ok=True)
            break


def build_service_metadata(services: list[dict], all_bins: set[Path]) -> list[dict]:
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


def build_manifest(
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
    from .build import find_stub

    stub = find_stub()
    import time

    t0 = time.time()

    with tempfile.TemporaryDirectory(prefix="xbin-build-") as tmp:
        tmp_path = Path(tmp)
        rt_dir = tmp_path / "runtime"
        app_dir_layer = tmp_path / "app"
        rt_dir.mkdir()
        app_dir_layer.mkdir()

        all_bins = collect_service_bins(services, verbose)
        copy_service_layers(all_bins, rt_dir, verbose)
        write_etc(rt_dir)

        copy_app_files(app_dir, app_dir_layer)
        install_manifest_pip(app_dir, services, tmp_path, app_dir_layer, verbose)
        (rt_dir / "data" / "db").mkdir(parents=True, exist_ok=True)
        (rt_dir / "tmp").mkdir(parents=True, exist_ok=True)

        rt_tar = tar_deterministic(rt_dir)
        app_tar = tar_deterministic(app_dir_layer)

    rt_comp = compress_layer_cached(
        rt_tar, reuse=False, verbose=verbose, label="runtime layer"
    )
    app_comp = compress_layer_cached(
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
    meta_services = build_service_metadata(services, all_bins)
    meta_bytes = build_meta_json(
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
    size = assemble_xbin(out_path, stub, payload, meta_bytes, key_path)
    label = "signed" if key_path else "unsigned"
    print(
        f"[xbin] wrote {out_path} ({size/1e6:.1f}MB, {label}) in {time.time()-t0:.1f}s",
        file=sys.stderr,
    )
    return str(out_path)
