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
        epilog=(
            "examples:\n"
            "  xbin build ./myapp                   Build a standalone .xbin\n"
            "  xbin build ./myapp -o out.xbin       Build with custom output path\n"
            "  xbin build ./myapp --key key.key      Build and sign in one step\n"
            "  xbin build ./myapp --update           Incremental rebuild (reuse layers)\n"
            "  xbin run myapp.xbin                   Run a packed binary\n"
            "  xbin inspect myapp.xbin               Show layers, deps, and metadata\n"
            "  xbin doctor                           Check prerequisites\n"
            "\nexit codes:\n"
            "  0   success\n"
            "  1   operation failed (build error, bad input, etc.)\n"
            "  2   usage error (missing args, invalid flags)\n"
            "\nfull docs: https://github.com/xbin-org/xbin"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--version",
        action="version",
        version="%(prog)s 0.1.0",
    )
    parser.add_argument(
        "--no-color",
        action="store_true",
        help="disable colored output (also: set NO_COLOR env var)",
    )
    sub = parser.add_subparsers(dest="command", required=True)
    _SUBPARSERS: dict[str, argparse.ArgumentParser] = {}

    p_build = sub.add_parser("build", help="analyze an app and produce a .xbin")
    _SUBPARSERS["build"] = p_build
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
    p_build.add_argument(
        "--encrypt",
        action="store_true",
        help="encrypt payload with AES-256-GCM (requires --key for signing seed)",
    )
    p_build.add_argument(
        "--squashfs",
        action="store_true",
        help="use squashfs images instead of zstd(tar) for payload layers (v5 format, better compression)",
    )
    p_build.add_argument(
        "--target",
        choices=["aarch64", "x86_64"],
        help="cross-compile for this architecture (downloads vendored Python, "
        "rejects compiled extensions; stub must be pre-built for target)",
    )
    p_build.add_argument("-q", "--quiet", action="store_true")
    p_build.add_argument(
        "--redetect",
        action="store_true",
        help="force re-detection of dependencies (overwrite xbin.lock)",
    )
    p_build.add_argument(
        "--update",
        action="store_true",
        help="incremental rebuild: reuse unchanged layers from existing .xbin",
    )

    p_run = sub.add_parser("run", help="run a .xbin file")
    _SUBPARSERS["run"] = p_run
    p_run.add_argument("file", help=".xbin file")
    p_run.add_argument(
        "args", nargs=argparse.REMAINDER, help="arguments passed to the app"
    )

    p_inspect = sub.add_parser("inspect", help="show .xbin contents")
    _SUBPARSERS["inspect"] = p_inspect
    p_inspect.add_argument("file", help=".xbin file")
    p_inspect.add_argument(
        "--json",
        action="store_true",
        help="output as JSON",
    )

    p_keygen = sub.add_parser("keygen", help="generate an Ed25519 signing keypair")
    _SUBPARSERS["keygen"] = p_keygen
    p_keygen.add_argument(
        "--key-dir",
        default=None,
        help="output directory for key files (default: $XDG_DATA_HOME/xbin/keys)",
    )
    p_keygen.add_argument("-q", "--quiet", action="store_true")

    p_sign = sub.add_parser("sign", help="sign a .xbin file (in-place, v3 footer)")
    _SUBPARSERS["sign"] = p_sign
    p_sign.add_argument("file", help=".xbin file to sign")
    p_sign.add_argument(
        "--key", help="path to signing key (default: first .key in keys directory)"
    )
    p_sign.add_argument("-q", "--quiet", action="store_true")

    p_verify = sub.add_parser("verify", help="verify a .xbin file's Ed25519 signature")
    _SUBPARSERS["verify"] = p_verify
    p_verify.add_argument("file", help=".xbin file to verify")
    p_verify.add_argument(
        "--trusted-dir",
        help="trusted keys directory " "(default: $XDG_DATA_HOME/xbin/trusted-keys)",
    )
    p_verify.add_argument("-q", "--quiet", action="store_true")

    p_trust = sub.add_parser(
        "trust", help="copy a .pub key into the trusted-keys directory"
    )
    _SUBPARSERS["trust"] = p_trust
    p_trust.add_argument("pubkey", help="path to a 32-byte Ed25519 public key file")
    p_trust.add_argument(
        "--trusted-dir",
        default=None,
        help="trusted keys directory (default: $XDG_DATA_HOME/xbin/trusted-keys)",
    )
    p_trust.add_argument("-q", "--quiet", action="store_true")

    p_clean = sub.add_parser("clean", help="clean local cache (~/.cache/xbin)")
    _SUBPARSERS["clean"] = p_clean
    p_clean.add_argument(
        "--all", action="store_true", help="remove all cache (including build cache)"
    )
    p_clean.add_argument(
        "-f",
        "--force",
        action="store_true",
        help="skip confirmation prompt (for use in scripts)",
    )

    p_selftest = sub.add_parser(
        "selftest", help="launch a .xbin in an ephemeral sandbox to confirm it starts"
    )
    _SUBPARSERS["selftest"] = p_selftest
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

    p_doctor = sub.add_parser(
        "doctor", help="check that all prerequisites are installed"
    )
    _SUBPARSERS["doctor"] = p_doctor
    p_doctor.add_argument("-q", "--quiet", action="store_true")
    p_doctor.add_argument(
        "--json",
        action="store_true",
        help="output as JSON",
    )
    p_doctor.add_argument(
        "--fix",
        action="store_true",
        help="attempt to install missing prerequisites automatically",
    )
    p_doctor.add_argument(
        "-f",
        "--force",
        action="store_true",
        help="skip confirmation prompt (for use with --fix in scripts)",
    )

    p_help = sub.add_parser(
        "help",
        help="show help for a command",
    )
    _SUBPARSERS["help"] = p_help
    p_help.add_argument(
        "command_name",
        nargs="?",
        default=None,
        help="command to get help for (omit for general help)",
    )

    args = parser.parse_args(argv)

    from ._color import init as _init_color

    _init_color(no_color=getattr(args, "no_color", False))

    _verbose = getattr(args, "quiet", False) is False and sys.stderr.isatty()

    try:
        if args.command == "help":
            if args.command_name is None:
                parser.print_help()
            elif args.command_name in _SUBPARSERS:
                _SUBPARSERS[args.command_name].print_help()
            else:
                print(f"unknown command: {args.command_name}", file=sys.stderr)
                return 1
            return 0

        if args.command == "build":
            from .build import build

            _warn_multi_service_compose(Path(args.app).resolve(), verbose=_verbose)
            build(
                args.app,
                args.output,
                key_path=args.key,
                isolation=args.isolation,
                seccomp=args.seccomp,
                encrypt=args.encrypt,
                squashfs=args.squashfs,
                verbose=_verbose,
                redetect=args.redetect,
                target=args.target,
                update=args.update,
            )
            return 0

        if args.command == "run":
            path = os.path.abspath(args.file)
            os.execv(path, [path, *args.args])  # replaces the process
            return 0  # unreachable

        if args.command == "inspect":
            from .inspect import inspect

            inspect(args.file, json_output=args.json)
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

            clean(all_entries=args.all, force=args.force)
            return 0

        if args.command == "selftest":
            from .selftest import selftest

            return selftest(
                args.file,
                mode=args.mode,
                timeout=args.timeout,
                probe=args.probe,
                verbose=_verbose,
            )

        if args.command == "doctor":
            from .doctor import doctor

            return doctor(
                verbose=_verbose,
                json_output=args.json,
                fix=args.fix,
                force=args.force,
            )
    except (
        FileNotFoundError,
        NotADirectoryError,
        PermissionError,
        ValueError,
        FileExistsError,
        RuntimeError,
    ) as e:
        from ._color import red as _red

        print(f"[xbin] {_red(f'error: {e}')}", file=sys.stderr)
        return 1

    return 1


if __name__ == "__main__":
    sys.exit(main())
