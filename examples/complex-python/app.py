"""Multi-file Python app: local package + vendored site-packages.

`requirements.txt` is comment-only so packaging must not invoke pip; deps
come from `site-packages/` (and `lib/` is part of the app tree).
"""

from __future__ import annotations

import os
import sys

# Vendored marker (must land on PYTHONPATH via staged site-packages).
import vendor_mark  # type: ignore[import-not-found]

from lib.greet import greet


def main() -> None:
    name = os.environ.get("COMPLEX_NAME", "daedalus")
    print(greet(name))
    print(f"vendor={vendor_mark.MARKER}")
    print(f"python={sys.version.split()[0]}")


if __name__ == "__main__":
    main()
