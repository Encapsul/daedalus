"""CLI xbin — point d'entrée des commandes build / run / inspect.

Stdlib only (argparse) pour zéro friction d'installation. click/rich viendront
en polish une fois le MVP validé.
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

    p_build = sub.add_parser("build", help="analyse une app et produit un .xbin")
    p_build.add_argument("app", help="répertoire de l'application")
    p_build.add_argument("-o", "--output", help="chemin de sortie (défaut: <name>.xbin)")
    p_build.add_argument("-q", "--quiet", action="store_true")

    p_run = sub.add_parser("run", help="lance un .xbin")
    p_run.add_argument("file", help="fichier .xbin")
    p_run.add_argument("args", nargs=argparse.REMAINDER, help="arguments passés à l'app")

    p_inspect = sub.add_parser("inspect", help="affiche le contenu d'un .xbin")
    p_inspect.add_argument("file", help="fichier .xbin")

    p_clean = sub.add_parser("clean", help="nettoie le cache local (~/.cache/xbin)")
    p_clean.add_argument("--all", action="store_true", help="supprime tout le cache")

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
    except (FileNotFoundError, NotADirectoryError, ValueError) as e:
        print(f"[xbin] error: {e}", file=sys.stderr)
        return 1

    return 1


if __name__ == "__main__":
    sys.exit(main())
