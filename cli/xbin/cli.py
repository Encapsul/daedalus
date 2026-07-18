"""xbin CLI — entry point for build / run / inspect / clean commands.

Stdlib only (argparse) for zero-install friction. click/rich can be added
for polish once the MVP is validated.
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path


def _parse_compose_services(app_dir: Path) -> list[dict[str, str]] | None:
    """Parse docker-compose.yml for service names and their kind (build/image).

    Returns None if no compose file, only one service, or file is unparseable.
    Each dict has keys 'name' and 'kind' ('build', 'image', or 'unknown').
    """
    for name in ("docker-compose.yml", "docker-compose.yaml"):
        compose_path = app_dir / name
        if compose_path.is_file():
            break
    else:
        return None

    try:
        lines = compose_path.read_text(encoding="utf-8").splitlines()
    except OSError:
        return None

    services: list[dict[str, str]] = []
    in_services = False
    current_service: str | None = None
    current_kind: str = "unknown"

    for line in lines:
        if not line or line.lstrip().startswith("#"):
            continue

        indent = len(line) - len(line.lstrip())

        if indent == 0 and line.strip() == "services:":
            in_services = True
            continue

        if in_services:
            if indent == 0 and line.strip():
                break  # left the services block

            if indent == 2:
                # Save previous service if any.
                if current_service is not None:
                    services.append({"name": current_service, "kind": current_kind})
                # New service name.
                m = re.match(r"^\s{2}(\S+)\s*:\s*$", line)
                if m:
                    current_service = m.group(1)
                    current_kind = "unknown"
                else:
                    current_service = None
                continue

            if indent == 4 and current_service is not None:
                stripped = line.strip()
                if stripped.startswith("build:") or stripped == "build: >":
                    current_kind = "build"
                elif stripped.startswith("image:"):
                    current_kind = "image"

    # Save last service.
    if current_service is not None:
        services.append({"name": current_service, "kind": current_kind})

    if len(services) <= 1:
        return None

    return services


def _warn_multi_service_compose(app_dir: Path, *, verbose: bool) -> None:
    """Warn if docker-compose.yml defines multiple services. Informational only."""
    services = _parse_compose_services(app_dir)
    if services is None:
        return

    names = ", ".join(s["name"] for s in services)
    build_services = [s["name"] for s in services if s["kind"] == "build"]
    image_services = [s["name"] for s in services if s["kind"] == "image"]

    if verbose:
        print(
            f"[xbin] warning: docker-compose.yml defines multiple services: {names}",
            file=sys.stderr,
        )
        print(
            "[xbin]          xbin packages a single process per .xbin file",
            file=sys.stderr,
        )
        if len(build_services) == 1:
            print(
                f"[xbin]          service '{build_services[0]}' uses build: "
                "(may be independently packageable)",
                file=sys.stderr,
            )
        elif len(build_services) > 1:
            bnames = ", ".join(f"'{n}'" for n in build_services)
            print(
                f"[xbin]          services {bnames} use build: "
                "(may be independently packageable)",
                file=sys.stderr,
            )
        if image_services:
            inames = ", ".join(f"'{n}'" for n in image_services)
            print(
                f"[xbin]          services {inames} use image: (likely dependencies)",
                file=sys.stderr,
            )
        print(
            "[xbin]          continuing build — xbin will only analyze the app directory",
            file=sys.stderr,
        )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="xbin",
        description="Ship your web app like a binary. Run anywhere.",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    p_build = sub.add_parser("build", help="analyze an app and produce a .xbin")
    p_build.add_argument("app", help="application directory")
    p_build.add_argument("-o", "--output", help="output path (default: <name>.xbin)")
    p_build.add_argument("--key", help="sign the .xbin with this Ed25519 key")
    p_build.add_argument(
        "--isolation",
        type=int,
        default=0,
        choices=[0, 1, 2],
        help="isolation level: 0=LD_LIBRARY_PATH, 1=chroot, 2=user namespaces (default: 0)",
    )
    p_build.add_argument(
        "--seccomp",
        action="store_true",
        help="install seccomp-bpf denylist (blocks dangerous syscalls, requires --isolation 2)",
    )
    p_build.add_argument("-q", "--quiet", action="store_true")
    p_build.add_argument(
        "--redetect",
        action="store_true",
        help="force re-detection of dependencies (overwrite xbin.lock)",
    )

    p_run = sub.add_parser("run", help="run a .xbin file")
    p_run.add_argument("file", help=".xbin file")
    p_run.add_argument(
        "args", nargs=argparse.REMAINDER, help="arguments passed to the app"
    )

    p_inspect = sub.add_parser("inspect", help="show .xbin contents")
    p_inspect.add_argument("file", help=".xbin file")

    p_keygen = sub.add_parser("keygen", help="generate an Ed25519 signing keypair")
    p_keygen.add_argument(
        "--key-dir",
        default="~/.xbin/keys",
        help="output directory for key files (default: ~/.xbin/keys)",
    )
    p_keygen.add_argument("-q", "--quiet", action="store_true")

    p_sign = sub.add_parser("sign", help="sign a .xbin file (in-place, v3 footer)")
    p_sign.add_argument("file", help=".xbin file to sign")
    p_sign.add_argument(
        "--key", help="path to signing key (default: first .key in ~/.xbin/keys/)"
    )
    p_sign.add_argument("-q", "--quiet", action="store_true")

    p_verify = sub.add_parser("verify", help="verify a .xbin file's Ed25519 signature")
    p_verify.add_argument("file", help=".xbin file to verify")
    p_verify.add_argument(
        "--trusted-dir",
        help="trusted keys directory " "(default: ~/.xbin/trusted-keys/)",
    )
    p_verify.add_argument("-q", "--quiet", action="store_true")

    p_trust = sub.add_parser(
        "trust", help="copy a .pub key into the trusted-keys directory"
    )
    p_trust.add_argument("pubkey", help="path to a 32-byte Ed25519 public key file")
    p_trust.add_argument(
        "--trusted-dir",
        default="~/.xbin/trusted-keys",
        help="trusted keys directory (default: ~/.xbin/trusted-keys/)",
    )
    p_trust.add_argument("-q", "--quiet", action="store_true")

    p_clean = sub.add_parser("clean", help="clean local cache (~/.cache/xbin)")
    p_clean.add_argument(
        "--all", action="store_true", help="remove all cache (including build cache)"
    )

    p_selftest = sub.add_parser(
        "selftest", help="launch a .xbin in an ephemeral sandbox to confirm it starts"
    )
    p_selftest.add_argument("file", help=".xbin file to test")
    p_selftest.add_argument(
        "--mode",
        choices=["auto", "server", "cli"],
        default="auto",
        help="detection mode: auto (default), server (expect liveness), cli (expect exit 0)",
    )
    p_selftest.add_argument(
        "--timeout",
        type=int,
        default=3,
        help="observation window in seconds after initial 2s crash check (default: 3)",
    )
    p_selftest.add_argument(
        "--probe",
        help="HTTP health check URL (e.g. http://127.0.0.1:8080/). "
        "If set, verifies the app is actually serving — returns exit 2 on failure.",
    )
    p_selftest.add_argument("-q", "--quiet", action="store_true")

    args = parser.parse_args(argv)

    try:
        if args.command == "build":
            from .build import build

            _warn_multi_service_compose(
                Path(args.app).resolve(), verbose=not args.quiet
            )
            build(
                args.app,
                args.output,
                key_path=args.key,
                isolation=args.isolation,
                seccomp=args.seccomp,
                verbose=not args.quiet,
                redetect=args.redetect,
            )
            return 0

        if args.command == "run":
            path = os.path.abspath(args.file)
            os.execv(path, [path, *args.args])  # replaces the process
            return 0  # unreachable

        if args.command == "inspect":
            from .inspect import inspect

            inspect(args.file)
            return 0

        if args.command == "keygen":
            from .keygen import keygen

            keygen(args)
            return 0

        if args.command == "sign":
            from .sign import sign

            sign(args)
            return 0

        if args.command == "verify":
            from .verify import verify

            verify(args)
            return 0

        if args.command == "trust":
            from .trust import trust

            trust(args)
            return 0

        if args.command == "clean":
            from .clean import clean

            clean(all_entries=args.all)
            return 0

        if args.command == "selftest":
            from .selftest import selftest

            return selftest(
                args.file,
                mode=args.mode,
                timeout=args.timeout,
                probe=args.probe,
                verbose=not args.quiet,
            )
    except (
        FileNotFoundError,
        NotADirectoryError,
        PermissionError,
        ValueError,
        FileExistsError,
        RuntimeError,
    ) as e:
        print(f"[xbin] error: {e}", file=sys.stderr)
        return 1

    return 1


if __name__ == "__main__":
    sys.exit(main())
