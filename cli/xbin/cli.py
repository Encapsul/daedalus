"""xbin CLI — entry point for build / run / inspect / clean commands.

Stdlib only (argparse) for zero-install friction. click/rich can be added
for polish once the MVP is validated.
"""

from __future__ import annotations

import argparse
import os
import sys


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

    args = parser.parse_args(argv)

    try:
        if args.command == "build":
            from .build import build

            build(
                args.app,
                args.output,
                key_path=args.key,
                isolation=args.isolation,
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
