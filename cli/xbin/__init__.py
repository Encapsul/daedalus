"""xbin — package any web/server app into a single self-extracting executable."""

from importlib.metadata import PackageNotFoundError, version

try:
    __version__ = version("xbin")
except PackageNotFoundError:
    __version__ = "0.0.0-dev"
