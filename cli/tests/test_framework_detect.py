"""Tests for framework auto-detect improvements."""

from __future__ import annotations

import json
from pathlib import Path

from xbin.runtimes.node import _detect_framework
from xbin.runtimes.python import PythonRuntime


def _make_pkg(app_dir: Path, deps: dict[str, str]) -> None:
    app_dir.mkdir(parents=True, exist_ok=True)
    pkg = {"name": "test-app", "dependencies": deps}
    (app_dir / "package.json").write_text(json.dumps(pkg))


class TestNodeFrameworks:
    def test_express_by_dep(self, tmp_path: Path) -> None:
        _make_pkg(tmp_path, {"express": "^4"})
        assert _detect_framework(tmp_path) == "express"

    def test_fastify_by_dep(self, tmp_path: Path) -> None:
        _make_pkg(tmp_path, {"fastify": "^4"})
        assert _detect_framework(tmp_path) == "fastify"

    def test_hono_by_dep(self, tmp_path: Path) -> None:
        _make_pkg(tmp_path, {"hono": "^4"})
        assert _detect_framework(tmp_path) == "hono"

    def test_remix_by_dep(self, tmp_path: Path) -> None:
        _make_pkg(tmp_path, {"@remix-run/node": "^2"})
        assert _detect_framework(tmp_path) == "remix"

    def test_sveltekit_by_dep(self, tmp_path: Path) -> None:
        _make_pkg(tmp_path, {"@sveltejs/kit": "^2"})
        assert _detect_framework(tmp_path) == "sveltekit"

    def test_nextjs_by_config(self, tmp_path: Path) -> None:
        _make_pkg(tmp_path, {})
        (tmp_path / "next.config.mjs").write_text("export default {}")
        assert _detect_framework(tmp_path) == "nextjs"

    def test_remix_by_config(self, tmp_path: Path) -> None:
        _make_pkg(tmp_path, {})
        (tmp_path / "remix.config.js").write_text("module.exports = {}")
        assert _detect_framework(tmp_path) == "remix"

    def test_sveltekit_by_config(self, tmp_path: Path) -> None:
        _make_pkg(tmp_path, {})
        (tmp_path / "svelte.config.js").write_text("export default {}")
        assert _detect_framework(tmp_path) == "sveltekit"

    def test_unknown_framework(self, tmp_path: Path) -> None:
        _make_pkg(tmp_path, {"lodash": "^4"})
        assert _detect_framework(tmp_path) is None

    def test_no_package_json(self, tmp_path: Path) -> None:
        assert _detect_framework(tmp_path) is None


class TestPythonFrameworks:
    def test_fastapi_detection(self, tmp_path: Path) -> None:
        app_dir = tmp_path / "app"
        app_dir.mkdir()
        (app_dir / "main.py").write_text(
            "from fastapi import FastAPI\napp = FastAPI()\n"
        )
        rt = PythonRuntime()
        plan = rt.detect(app_dir)
        assert plan is not None
        assert plan.runtime == "python"
        # Entry should reference main.py or uvicorn (if installed)
        entry_str = " ".join(plan.entrypoint)
        assert "main" in entry_str or "uvicorn" in entry_str

    def test_flask_detection(self, tmp_path: Path) -> None:
        app_dir = tmp_path / "app"
        app_dir.mkdir()
        (app_dir / "app.py").write_text(
            "from flask import Flask\napp = Flask(__name__)\n"
        )
        rt = PythonRuntime()
        plan = rt.detect(app_dir)
        assert plan is not None
        assert plan.runtime == "python"

    def test_django_still_works(self, tmp_path: Path) -> None:
        app_dir = tmp_path / "app"
        (app_dir / "myproject").mkdir(parents=True)
        (app_dir / "manage.py").write_text("#!/usr/bin/env python")
        (app_dir / "myproject" / "wsgi.py").write_text(
            "application = None"
        )
        rt = PythonRuntime()
        plan = rt.detect(app_dir)
        assert plan is not None
        assert plan.runtime == "python"
        # Entry should reference wsgi or manage.py or gunicorn (if installed)
        entry_str = " ".join(plan.entrypoint)
        assert "wsgi" in entry_str or "manage.py" in entry_str or "gunicorn" in entry_str

    def test_generic_python_fallback(self, tmp_path: Path) -> None:
        app_dir = tmp_path / "app"
        app_dir.mkdir()
        (app_dir / "main.py").write_text("print('hello')\n")
        rt = PythonRuntime()
        plan = rt.detect(app_dir)
        assert plan is not None
        assert plan.runtime == "python"
        assert "main.py" in " ".join(plan.entrypoint)

    def test_no_python_app(self, tmp_path: Path) -> None:
        app_dir = tmp_path / "app"
        app_dir.mkdir()
        rt = PythonRuntime()
        assert rt.detect(app_dir) is None
