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


class TestDjangoDetection:
    def test_detect_django_with_wsgi(self, python_runtime, tmp_path):
        (tmp_path / "manage.py").write_text("#!/usr/bin/env python")
        project_dir = tmp_path / "myproject"
        project_dir.mkdir()
        (project_dir / "wsgi.py").write_text("application = None")
        with (
            patch("xbin.runtimes.python.shutil.which") as mock_which,
            patch("xbin.runtimes.python.sys", _mock_sys()),
        ):
            mock_which.side_effect = lambda cmd: {
                "python3": "/usr/bin/python3",
                "gunicorn": "/usr/bin/gunicorn",
            }.get(cmd)
            plan = python_runtime.detect(tmp_path)
        assert plan is not None
        assert plan.runtime == "python"
        assert any("gunicorn" in part for part in plan.entrypoint)
        assert any("myproject.wsgi" in part for part in plan.entrypoint)

    def test_detect_django_with_asgi(self, python_runtime, tmp_path):
        (tmp_path / "manage.py").write_text("#!/usr/bin/env python")
        project_dir = tmp_path / "myproject"
        project_dir.mkdir()
        (project_dir / "asgi.py").write_text("application = None")
        with (
            patch("xbin.runtimes.python.shutil.which") as mock_which,
            patch("xbin.runtimes.python.sys", _mock_sys()),
        ):
            mock_which.side_effect = lambda cmd: {
                "python3": "/usr/bin/python3",
                "uvicorn": "/usr/bin/uvicorn",
            }.get(cmd)
            plan = python_runtime.detect(tmp_path)
        assert plan is not None
        assert any("uvicorn" in part for part in plan.entrypoint)
        assert any("myproject.asgi" in part for part in plan.entrypoint)

    def test_django_fallback_to_manage_py(self, python_runtime, tmp_path):
        (tmp_path / "manage.py").write_text("#!/usr/bin/env python")
        project_dir = tmp_path / "myproject"
        project_dir.mkdir()
        (project_dir / "wsgi.py").write_text("application = None")
        with (
            patch("xbin.runtimes.python.shutil.which") as mock_which,
            patch("xbin.runtimes.python.sys", _mock_sys()),
        ):
            mock_which.side_effect = lambda cmd: {
                "python3": "/usr/bin/python3",
            }.get(cmd)
            plan = python_runtime.detect(tmp_path)
        assert plan is not None
        assert any("manage.py" in part for part in plan.entrypoint)
        assert any("runserver" in part for part in plan.entrypoint)

    def test_django_no_wsgi_no_asgi(self, python_runtime, tmp_path):
        (tmp_path / "manage.py").write_text("#!/usr/bin/env python")
        # No wsgi.py or asgi.py — should fall through to generic Python
        (tmp_path / "app.py").write_text("print('hello')")
        with (
            patch("xbin.runtimes.python.shutil.which", return_value="/usr/bin/python3"),
            patch("xbin.runtimes.python.sys", _mock_sys()),
        ):
            plan = python_runtime.detect(tmp_path)
        assert plan is not None
        # Should detect as generic Python, not Django
        assert any("app.py" in part for part in plan.entrypoint)
