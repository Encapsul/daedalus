"""Unit tests for Go runtime detection."""

from pathlib import Path
from unittest.mock import patch

import pytest

from xbin.runtimes.go import GoRuntime


@pytest.fixture
def go_runtime():
    return GoRuntime()


class TestGoRuntime:
    def test_detect_with_go_mod(self, go_runtime, tmp_path):
        go_mod = tmp_path / "go.mod"
        go_mod.write_text("module example.com/myapp\n\ngo 1.21\n")
        main_go = tmp_path / "main.go"
        main_go.write_text("package main\n\nfunc main() {}\n")

        with (
            patch("xbin.runtimes.go.shutil.which", return_value="/usr/bin/go"),
            patch("xbin.runtimes.go.subprocess.run"),
        ):
            plan = go_runtime.detect(tmp_path)
            assert plan is not None
            assert plan.runtime == "go"
            assert plan.interpreter_host.name == "go"
            assert plan.entrypoint == ["/app/app"]

    def test_no_detect_without_go_mod(self, go_runtime, tmp_path):
        main_go = tmp_path / "main.go"
        main_go.write_text("package main\n\nfunc main() {}\n")

        plan = go_runtime.detect(tmp_path)
        assert plan is None

    def test_detect_no_go_on_path(self, go_runtime, tmp_path):
        go_mod = tmp_path / "go.mod"
        go_mod.write_text("module example.com/myapp\n\ngo 1.21\n")

        with (
            patch("xbin.runtimes.go.shutil.which", return_value=None),
            pytest.raises(ValueError, match="no go on PATH"),
        ):
            go_runtime.detect(tmp_path)

    def test_supports_cross_true(self, go_runtime):
        assert go_runtime.supports_cross() is True
