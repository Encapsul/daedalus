import os
import sys

try:
    os.environ.setdefault("XBIN_ORIG_CWD", os.getcwd())
except OSError:
    pass

if os.environ.get("XBIN_VERBOSE"):
    orig = os.environ.get("XBIN_ORIG_CWD", "(not set)")
    print(f"[app.py debug] XBIN_ORIG_CWD={orig}", file=sys.stderr)
    print(f"[app.py debug] argv={sys.argv}", file=sys.stderr)

here = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, here)
os.environ.setdefault("XBIN_STUB", os.path.join(here, "xbin-stub"))

from xbin.cli import main
sys.exit(main())
