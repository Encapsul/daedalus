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
from .dotenv import load_dotenv
from .encrypt import encrypt_payload
from .layers import build_layers
from .manifest import build_manifest
from .persistent import get_persist_env
from .pkgmgr import detect_pkgmgr, install_deps

XBIN_VERSION = "0.1.0"

_IGNORED_APP_DIRS = {".venv", "venv", "site-packages", "node_modules", ".git"}

_LOCK_FILES = [
    "requirements.txt",
    "uv.lock",
    "poetry.lock",
    "Pipfile.lock",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lockb",
]


def _hash_lock_file(app_dir: Path) -> str:
    """Hash the first matching lock file for change detection."""
    for name in _LOCK_FILES:
        p = app_dir / name
        if p.is_file() and p.stat().st_size > 0:
            return hashlib.sha256(p.read_bytes()).hexdigest()
    return ""


def _hash_app_files(app_dir: Path) -> str:
    """Compute a SHA-256 hash of all app files for change detection.

    Excludes .venv, venv, site-packages, node_modules, .git.
    """
    h = hashlib.sha256()
    for p in sorted(app_dir.rglob("*")):
        if p.is_dir():
            continue
        if any(part in _IGNORED_APP_DIRS for part in p.parts):
            continue
        if p.is_symlink():
            h.update(os.readlink(p).encode())
        elif p.is_file():
            h.update(p.read_bytes())
    return h.hexdigest()


def _read_existing_xbin(xbin_path: Path, verbose: bool) -> tuple[bytes, dict] | None:
    """Read the runtime layer blob and metadata from an existing .xbin.

    Returns (runtime_blob, metadata_dict) or None if not readable.
    """
    from . import _format as fmt

    try:
        footer = fmt.read_footer(str(xbin_path))
        with open(xbin_path, "rb") as f:
            meta_bytes = fmt.read_at(f, footer.meta_offset, footer.meta_size)
        import json

        meta = json.loads(meta_bytes)
        layers = meta.get("layers", [])
        if len(layers) < 2:
            return None
        rt_layer = layers[0]
        with open(xbin_path, "rb") as f:
            rt_blob = fmt.read_at(f, rt_layer["offset"], rt_layer["csize"])
        return rt_blob, meta
    except (ValueError, KeyError, OSError) as e:
        if verbose:
            print(f"[xbin] could not read existing .xbin: {e}", file=sys.stderr)
        return None


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
    update: bool = False,
    no_install: bool = False,
    env_file: str | None = None,
    version: str = "",
    author: str = "",
    description: str = "",
    license: str = "",
    persist: bool = False,
    include: list[str] | None = None,
    tree_shake: bool = False,
    minify: bool = False,
    health_port: int = 0,
    otel_endpoint: str | None = None,
    otel_protocol: str = "grpc",
    cron_tasks: list[str] | None = None,
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

    # --- Load .env file and merge into plan.env ---
    env_file_path: Path | None = None
    if env_file:
        dotenv_env = load_dotenv(app_dir, env_file, verbose=verbose)
        plan.env.update(dotenv_env)
        # Resolve env_file_path for copying into app layer.
        candidate = Path(env_file)
        if not candidate.is_absolute():
            candidate = app_dir / env_file
        if candidate.is_file():
            env_file_path = candidate

    # --- Persistent storage ---
    if persist:
        persist_env = get_persist_env(name)
        plan.env.update(persist_env)
        if verbose:
            print(
                f"  persistent storage: ~/.local/share/xbin/{name}/",
                file=sys.stderr,
            )

    # --- Resolve include paths ---
    include_paths: list[Path] = []
    if include:
        for inc in include:
            inc_path = Path(inc)
            if not inc_path.is_absolute():
                inc_path = app_dir / inc
            if not inc_path.exists():
                raise FileNotFoundError(f"--include: path not found: {inc}")
            include_paths.append(inc_path)
            if verbose:
                print(
                    f"  include: {inc_path.name} ({'dir' if inc_path.is_dir() else 'file'})",
                    file=sys.stderr,
                )

    # --- Tree-shaking: remove unused node_modules packages ---
    if tree_shake:
        from .treeshake import prune_node_modules

        removed = prune_node_modules(app_dir, verbose=verbose)
        if verbose:
            print(
                f"  tree-shake: removed {removed} unused package(s)",
                file=sys.stderr,
            )

    # --- Minification: shrink JS/TS/CSS ---
    if minify:
        from .minify import minify_app_dir

        minified = minify_app_dir(app_dir, verbose=verbose)
        if verbose:
            print(
                f"  minify: minified {minified} file(s)",
                file=sys.stderr,
            )

    # --- OpenTelemetry setup ---
    if otel_endpoint:
        from .otel import build_otel_env

        otel_env = build_otel_env(
            service_name=plan.env.get("XBIN_APP_NAME", "app"),
            version=version,
            endpoint=otel_endpoint,
            protocol=otel_protocol,
        )
        plan.env.update(otel_env)
        if verbose:
            print(
                f"  otel: endpoint={otel_endpoint} protocol={otel_protocol}",
                file=sys.stderr,
            )

    # --- Cron/scheduled tasks ---
    if cron_tasks:
        from .cron import build_cron_env

        parsed_tasks = []
        for ct in cron_tasks:
            if ":" not in ct:
                raise ValueError(f"--cron format: NAME:SCHEDULE (got '{ct}')")
            name, _, schedule = ct.partition(":")
            parsed_tasks.append({"name": name, "schedule": schedule})
        cron_env = build_cron_env(parsed_tasks)
        plan.env.update(cron_env)
        if verbose:
            print(
                f"  cron: {len(parsed_tasks)} task(s) registered",
                file=sys.stderr,
            )

    # --- Health check port ---
    if health_port:
        plan.env["XBIN_HEALTH_PORT"] = str(health_port)
        if verbose:
            print(
                f"  health: endpoint enabled on port {health_port}",
                file=sys.stderr,
            )

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

    # --- Package manager install (uv/poetry/pipenv/pip/pnpm/yarn/bun/npm) ---
    if not no_install:
        pm = detect_pkgmgr(app_dir, plan.runtime)
        if pm is not None:
            if verbose:
                print(
                    f"[xbin] installing dependencies via {pm.name}...", file=sys.stderr
                )
            install_deps(app_dir, pm, verbose)

    # --- Compute hashes for metadata (always, used by --update and stored) ---
    new_app_hash = _hash_app_files(app_dir)
    new_rt_hash = _hash_lock_file(app_dir)

    # --- Incremental update: reuse existing layers when possible ---
    reuse_rt_blob: bytes | None = None
    reuse_rt_usize: int = 0
    if update and out_path.is_file():
        existing = _read_existing_xbin(out_path, verbose)
        if existing is not None:
            old_rt_blob, old_meta = existing
            old_app_hash = old_meta.get("app_hash", "")
            old_rt_hash = old_meta.get("rt_deps_hash", "")
            old_layers = old_meta.get("layers", [])
            old_rt_usize = old_layers[0].get("usize", 0) if old_layers else 0

            if old_app_hash == new_app_hash and old_rt_hash == new_rt_hash:
                if verbose:
                    print(
                        "[xbin] everything up to date, nothing to rebuild",
                        file=sys.stderr,
                    )
                return str(out_path)
            elif old_rt_hash == new_rt_hash and old_app_hash != new_app_hash:
                if verbose:
                    print("[xbin] app changed, reusing runtime layer", file=sys.stderr)
                reuse_rt_blob = old_rt_blob
                reuse_rt_usize = old_rt_usize
            else:
                if verbose:
                    reason = (
                        "runtime deps changed"
                        if old_rt_hash != new_rt_hash
                        else "first update"
                    )
                    print(f"[xbin] {reason}, full rebuild", file=sys.stderr)
    if squashfs:
        if reuse_rt_blob is not None:
            # Reuse existing runtime squashfs blob, only rebuild app layer.
            import tempfile

            from .layers import build_app_layer, mksquashfs

            with tempfile.TemporaryDirectory(prefix="xbin-build-") as tmp:
                app_dir_layer = Path(tmp) / "app"
                app_dir_layer.mkdir()
                build_app_layer(
                    app_dir,
                    plan,
                    app_dir_layer,
                    verbose,
                    env_file_path=env_file_path,
                    include_paths=include_paths,
                )
                app_sqfs = mksquashfs(app_dir_layer)
            rt_sqfs = reuse_rt_blob
            if verbose:
                print(
                    f"  runtime layer: reused ({len(rt_sqfs)/1e6:.1f}MB)",
                    file=sys.stderr,
                )
                print(
                    f"  app layer: {len(app_sqfs)/1e6:.1f}MB (squashfs)",
                    file=sys.stderr,
                )
        else:
            rt_sqfs, app_sqfs = build_layers(
                app_dir,
                plan,
                verbose,
                squashfs=True,
                cross_python_root=cross_root,
                target_arch=target,
                env_file_path=env_file_path,
                include_paths=include_paths,
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
        rt_usize = reuse_rt_usize
        app_usize = 0
        if reuse_rt_blob is not None:
            # Reuse existing runtime blob, only rebuild app layer.
            import tempfile

            from .layers import build_app_layer, tar_deterministic

            with tempfile.TemporaryDirectory(prefix="xbin-build-") as tmp:
                app_dir_layer = Path(tmp) / "app"
                app_dir_layer.mkdir()
                build_app_layer(
                    app_dir,
                    plan,
                    app_dir_layer,
                    verbose,
                    env_file_path=env_file_path,
                    include_paths=include_paths,
                )
                app_tar = tar_deterministic(app_dir_layer)
            from .layers import compress_layer_cached

            app_comp = compress_layer_cached(
                app_tar, reuse=False, verbose=verbose, label="app layer"
            )
            rt_comp = reuse_rt_blob
            app_usize = len(app_tar)
            if verbose:
                print(
                    f"  runtime layer: reused ({len(rt_comp)/1e6:.1f}MB)",
                    file=sys.stderr,
                )
        else:
            rt_comp, app_comp, rt_tar, app_tar = build_layers(
                app_dir,
                plan,
                verbose,
                squashfs=False,
                cross_python_root=cross_root,
                target_arch=target,
                env_file_path=env_file_path,
                include_paths=include_paths,
            )
            rt_usize = len(rt_tar)
            app_usize = len(app_tar)
        stub_bytes = stub.read_bytes()
        rt_offset = len(stub_bytes)
        payload = rt_comp + app_comp
        layers = [
            {
                "kind": "runtime",
                "offset": rt_offset,
                "csize": len(rt_comp),
                "usize": rt_usize,
                "sha256": hashlib.sha256(rt_comp).hexdigest(),
            },
            {
                "kind": "app",
                "offset": rt_offset + len(rt_comp),
                "csize": len(app_comp),
                "usize": app_usize,
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
        app_hash=new_app_hash,
        rt_deps_hash=new_rt_hash,
        version=version,
        author=author,
        description=description,
        license=license,
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
