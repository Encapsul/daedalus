"""daedalus — package any app into a single self-extracting binary."""

from .launcher import VERSION, main

__version__ = VERSION

__all__ = ["VERSION", "main"]