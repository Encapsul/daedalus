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
    p_build.add_argument("-q", "--quiet", action="store_true")

    p_run = sub.add_parser("run", help="run a .xbin file")
    p_run.add_argument("file", help=".xbin file")
    p_run.add_argument("args", nargs=argparse.REMAINDER, help="arguments passed to the app")

    p_inspect = sub.add_parser("inspect", help="show .xbin contents")
    p_inspect.add_argument("file", help=".xbin file")

    p_clean = sub.add_parser("clean", help="clean local cache (~/.cache/xbin)")
    p_clean.add_argument("--all", action="store_true", help="remove all cache (including build cache)")

    args = parser.parse_args(argv)

    try:
        if args.command == "build":
            from .build import build
            build(args.app, args.output, verbose=not args.quiet)
            return 0

        if args.command == "run":
            path = os.path.abspath(args.file)
            os.execv(path, [path, *args.args])  # remplace le process
            return 0  # unreachable

        if args.command == "inspect":
            from .inspect import inspect
            inspect(args.file)
            return 0

        if args.command == "clean":
            from .clean import clean
            clean(all_entries=args.all)
            return 0
    except (FileNotFoundError, NotADirectoryError, PermissionError, ValueError) as e:
        print(f"[xbin] error: {e}", file=sys.stderr)
        return 1

    return 1


if __name__ == "__main__":
    sys.exit(main())
