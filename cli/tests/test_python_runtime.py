"""Unit tests for Python runtime detection."""

from types import SimpleNamespace
from unittest.mock import patch

import pytest

from xbin.runtimes.python import PythonRuntime


@pytest.fixture
def python_runtime():
    return PythonRuntime()


def _mock_sys():
    mock = SimpleNamespace()
    mock.base_prefix = "/usr"
    mock.version_info = SimpleNamespace(major=3, minor=12)
    mock.executable = "/usr/bin/python3"
    return mock


class TestPythonRuntime:
    def test_detect_app_py(self, python_runtime, tmp_path):
        (tmp_path / "app.py").write_text("print('hello')")
        with (
            patch("xbin.runtimes.python.shutil.which", return_value="/usr/bin/python3"),
            patch("xbin.runtimes.python.sys", _mock_sys()),
        ):
            plan = python_runtime.detect(tmp_path)
        assert plan is not None
        assert plan.runtime == "python"

    def test_detect_main_py(self, python_runtime, tmp_path):
        (tmp_path / "main.py").write_text("print('hello')")
        with (
            patch("xbin.runtimes.python.shutil.which", return_value="/usr/bin/python3"),
            patch("xbin.runtimes.python.sys", _mock_sys()),
        ):
            plan = python_runtime.detect(tmp_path)
        assert plan is not None
        assert plan.runtime == "python"

    def test_detect_dunder_main(self, python_runtime, tmp_path):
        (tmp_path / "__main__.py").write_text("print('hello')")
        with (
            patch("xbin.runtimes.python.shutil.which", return_value="/usr/bin/python3"),
            patch("xbin.runtimes.python.sys", _mock_sys()),
        ):
            plan = python_runtime.detect(tmp_path)
        assert plan is not None

    def test_detect_server_py(self, python_runtime, tmp_path):
        (tmp_path / "server.py").write_text("print('hello')")
        with (
            patch("xbin.runtimes.python.shutil.which", return_value="/usr/bin/python3"),
            patch("xbin.runtimes.python.sys", _mock_sys()),
        ):
            plan = python_runtime.detect(tmp_path)
        assert plan is not None

    def test_no_detect_without_python_files(self, python_runtime, tmp_path):
        (tmp_path / "readme.txt").write_text("hello")
        plan = python_runtime.detect(tmp_path)
        assert plan is None

    def test_detect_with_venv_site_packages(self, python_runtime, tmp_path):
        (tmp_path / "app.py").write_text("print('hello')")
        venv_sp = tmp_path / ".venv" / "lib" / "python3.12" / "site-packages"
        venv_sp.mkdir(parents=True)
        with (
            patch("xbin.runtimes.python.shutil.which", return_value="/usr/bin/python3"),
            patch("xbin.runtimes.python.sys", _mock_sys()),
        ):
            plan = python_runtime.detect(tmp_path)
        assert plan is not None
        assert len(plan.site_packages) == 1
        assert plan.env.get("PYTHONPATH") is not None

    def test_detect_with_vendored_site_packages(self, python_runtime, tmp_path):
        (tmp_path / "app.py").write_text("print('hello')")
        sp = tmp_path / "site-packages"
        sp.mkdir()
        with (
            patch("xbin.runtimes.python.shutil.which", return_value="/usr/bin/python3"),
            patch("xbin.runtimes.python.sys", _mock_sys()),
        ):
            plan = python_runtime.detect(tmp_path)
        assert plan is not None
        assert len(plan.site_packages) == 1

    def test_supports_cross_true(self, python_runtime):
        assert python_runtime.supports_cross() is True

    def test_env_sets_unbuffered(self, python_runtime, tmp_path):
        (tmp_path / "app.py").write_text("print('hello')")
        with (
            patch("xbin.runtimes.python.shutil.which", return_value="/usr/bin/python3"),
            patch("xbin.runtimes.python.sys", _mock_sys()),
        ):
            plan = python_runtime.detect(tmp_path)
        assert plan.env["PYTHONUNBUFFERED"] == "1"
        assert plan.env["PYTHONDONTWRITEBYTECODE"] == "1"
