"""Unit tests for the daedalus pip launcher (no network, no pip deps)."""

import io
import os
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from daedalus import launcher


def make_tarball(files: dict[str, bytes]) -> bytes:
    """Build an in-memory gzipped tarball mirroring a release archive layout."""
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz") as tar:
        for name, content in files.items():
            info = tarfile.TarInfo(name)
            info.size = len(content)
            tar.addfile(info, io.BytesIO(content))
    return buf.getvalue()


class DetectPlatformTest(unittest.TestCase):
    def test_known_host_maps(self):
        # macOS amd64 shorthand is covered; Windows via sys.platform is hard to
        # fake, so just assert the struct is well-formed on the running host.
        p = launcher.detect_platform()
        self.assertIn(p.os, ("linux", "darwin", "windows"))
        self.assertIn(p.arch, ("amd64", "arm64"))

    def test_asset_name(self):
        p = launcher.Platform("darwin", "amd64")
        self.assertEqual(
            launcher.release_url(p).rsplit("/", 1)[-1],
            f"daedalus_{launcher.VERSION}_darwin_amd64.tar.gz",
        )


class ChecksumTest(unittest.TestCase):
    def test_sha256(self):
        self.assertEqual(
            launcher.sha256_bytes(b"hello world"),
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9",
        )

    def test_expected_checksum_parse(self):
        lines = "".join([
            f"foo  daedalus_{launcher.VERSION}_darwin_amd64.tar.gz\n",
            f"bar  daedalus_{launcher.VERSION}_darwin_arm64.tar.gz\n",
        ])
        self.assertEqual(
            launcher.parse_checksums(
                lines, f"daedalus_{launcher.VERSION}_darwin_arm64.tar.gz"
            ),
            "bar",
        )

    def test_parse_missing_asset_fails(self):
        with self.assertRaises(SystemExit):
            launcher.parse_checksums("x  nope.tar.gz\n", "daedalus_missing")


class ExtractTest(unittest.TestCase):
    def test_extract_full_archive(self):
        content = make_tarball({
            "daedalus_0.7.1_darwin_amd64/daedalus": b"\x7fELFfake",
            "daedalus_0.7.1_darwin_amd64/daedalus-stub": b"\x7fELFstub",
            "daedalus_0.7.1_darwin_amd64/daedalus-crypto": b"crypto",
        })
        with tempfile.TemporaryDirectory() as td:
            dest = Path(td)
            binary = launcher.extract_daedalus(content, dest)
            self.assertEqual(binary, dest / "daedalus")
            self.assertEqual((dest / "daedalus").read_bytes(), b"\x7fELFfake")
            self.assertEqual((dest / "daedalus-stub").read_bytes(), b"\x7fELFstub")
            self.assertEqual((dest / "daedalus-crypto").read_bytes(), b"crypto")

    def test_extract_windows_exe(self):
        content = make_tarball({
            "daedalus_0.7.1_windows_amd64/daedalus.exe": b"MZexe",
            "daedalus_0.7.1_windows_amd64/daedalus-stub.exe": b"MZstub",
            "daedalus_0.7.1_windows_amd64/daedalus-crypto.exe": b"MZcrypto",
        })
        with tempfile.TemporaryDirectory() as td:
            dest = Path(td)
            binary = launcher.extract_daedalus(content, dest)
            self.assertEqual(binary, dest / "daedalus.exe")

    def test_extract_missing_binary_fails(self):
        content = make_tarball({"daedalus_0.7.1_darwin_amd64/README": b"nope"})
        with tempfile.TemporaryDirectory() as td:
            with self.assertRaises(SystemExit):
                launcher.extract_daedalus(content, Path(td))

    def test_unsafe_path_rejected(self):
        content = make_tarball({"daedalus_0.7.1_darwin_amd64/../../evil": b"x"})
        with tempfile.TemporaryDirectory() as td:
            with self.assertRaises(SystemExit):
                launcher.extract_daedalus(content, Path(td))


if __name__ == "__main__":
    unittest.main(verbosity=2)