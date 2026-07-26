"""Unit tests for native ELF binary runtime detection."""

import stat

import pytest

from xbin.runtimes.binary import BinaryRuntime


@pytest.fixture
def binary_runtime():
    return BinaryRuntime()


ELF_HEADER = b"\x7fELF" + b"\x00" * 100


class TestBinaryDetection:
    def test_detect_single_elf(self, binary_runtime, tmp_path):
        elf = tmp_path / "myapp"
        elf.write_bytes(ELF_HEADER)
        elf.chmod(elf.stat().st_mode | stat.S_IXUSR)
        plan = binary_runtime.detect(tmp_path)
        assert plan is not None
        assert plan.runtime == "binary"
        assert plan.interpreter_host is None
        assert "/app/myapp" in plan.entrypoint

    def test_no_detect_without_elf(self, binary_runtime, tmp_path):
        (tmp_path / "readme.txt").write_text("hello")
        plan = binary_runtime.detect(tmp_path)
        assert plan is None

    def test_no_detect_multiple_elfs(self, binary_runtime, tmp_path):
        for name in ("app1", "app2"):
            elf = tmp_path / name
            elf.write_bytes(ELF_HEADER)
            elf.chmod(elf.stat().st_mode | stat.S_IXUSR)
        plan = binary_runtime.detect(tmp_path)
        assert plan is None

    def test_no_detect_non_elf_executable(self, binary_runtime, tmp_path):
        script = tmp_path / "script"
        script.write_text("#!/bin/bash\necho hi")
        script.chmod(script.stat().st_mode | stat.S_IXUSR)
        plan = binary_runtime.detect(tmp_path)
        assert plan is None

    def test_supports_cross_false(self, binary_runtime):
        assert binary_runtime.supports_cross() is False

    def test_elf_with_no_permission(self, binary_runtime, tmp_path):
        elf = tmp_path / "myapp"
        elf.write_bytes(ELF_HEADER)
        plan = binary_runtime.detect(tmp_path)
        assert plan is None
