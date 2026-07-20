"""xbin build: analyze an app, build the rootfs, assemble the .xbin."""

from __future__ import annotations

import hashlib
import os
import sys
import time
from pathlib import Path

try:
    import tomllib
except ImportError:
    import tomli as tomllib  # type: ignore[no-redef]

from ._util import find_binary
from .analyzer import runtime
from .analyzer.dockerfile import detect_from_dockerfile
from .analyzer.fetch import fetch_deps
from .analyzer.lockfile import (
    detect_or_read_lock,
    write_lock_from_results,
)
from .analyzer.python_ast import detect_from_python_source, merge_deps
from .assembly import assemble_xbin, build_meta_json, read_signing_seed
from .cross import (
    cross_python_root,
    host_arch,
    is_cross_build,
    resolve_cross_python,
)
from .encrypt import encrypt_payload
from .layers import build_layers
from .manifest import build_manifest

XBIN_VERSION = "0.1.0"


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
        return build_manifest(app_dir, manifest, output, key_path, verbose)

    # --- Dependency detection (Features A/B/C) + lockfile ---
    locked_deps = detect_or_read_lock(app_dir, redetect=redetect, verbose=verbose)
    if locked_deps is not None:
        # Lock is fresh — use locked deps, skip detection.
        dep_list = locked_deps
    else:
        # No lock or stale — run full detection pipeline.
        if verbose:
            print("[xbin] detecting dependencies...", file=sys.stderr)
        dockerfile_deps = detect_from_dockerfile(app_dir)
        ast_deps = detect_from_python_source(app_dir)
        dep_list = merge_deps(dockerfile_deps, ast_deps)
        if dep_list:
            if verbose:
                n = len(dep_list)
                print(
                    f"[xbin] downloading {n} dependenc{'y' if n == 1 else 'ies'}...",
                    file=sys.stderr,
                )
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
        rt_sqfs, app_sqfs = build_layers(
            app_dir, plan, verbose, squashfs=True,
            cross_python_root=cross_root, target_arch=target,
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
        payload_format = "squashfs"
    else:
        rt_comp, app_comp, rt_tar, app_tar = build_layers(
            app_dir, plan, verbose, squashfs=False,
            cross_python_root=cross_root, target_arch=target,
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
        signing_seed = read_signing_seed(key_path)
        pre_enc_size = len(payload)
        payload, enc_meta = encrypt_payload(payload, signing_seed)
        crypto_meta = enc_meta
        if verbose:
            print(
                f"  encrypted: {pre_enc_size/1e6:.1f}MB -> {len(payload)/1e6:.1f}MB (AES-256-GCM)",
                file=sys.stderr,
            )

    meta_bytes = build_meta_json(
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
    size = assemble_xbin(
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
