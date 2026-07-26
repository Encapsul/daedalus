"""Tests for --include (data files) feature."""

from __future__ import annotations

import shutil
from pathlib import Path

from xbin.layers import build_app_layer
from xbin.runtimes import RuntimePlan


def _make_plan() -> RuntimePlan:
    return RuntimePlan(
        runtime="python",
        interpreter_host=Path("/usr/bin/python3"),
        entrypoint=["python3", "app.py"],
        cwd="/app",
        env={},
        extra_dirs_host=[],
        site_packages=[],
    )


class TestIncludeFiles:
    """Tests for --include data files in app layer."""

    def test_include_file(self, tmp_path: Path) -> None:
        app_dir = tmp_path / "app"
        app_dir.mkdir()
        (app_dir / "main.py").write_text("print('hello')")

        data_file = tmp_path / "data.txt"
        data_file.write_text("some data")

        layer = tmp_path / "layer"
        layer.mkdir()
        build_app_layer(
            app_dir,
            _make_plan(),
            layer,
            verbose=False,
            include_paths=[data_file],
        )
        assert (layer / "app" / "data.txt").read_text() == "some data"

    def test_include_directory(self, tmp_path: Path) -> None:
        app_dir = tmp_path / "app"
        app_dir.mkdir()
        (app_dir / "main.py").write_text("print('hello')")

        data_dir = tmp_path / "templates"
        data_dir.mkdir()
        (data_dir / "index.html").write_text("<h1>Hello</h1>")
        (data_dir / "style.css").write_text("body {}")

        layer = tmp_path / "layer"
        layer.mkdir()
        build_app_layer(
            app_dir,
            _make_plan(),
            layer,
            verbose=False,
            include_paths=[data_dir],
        )
        assert (layer / "app" / "templates" / "index.html").read_text() == "<h1>Hello</h1>"
        assert (layer / "app" / "templates" / "style.css").read_text() == "body {}"

    def test_include_multiple(self, tmp_path: Path) -> None:
        app_dir = tmp_path / "app"
        app_dir.mkdir()
        (app_dir / "main.py").write_text("print('hello')")

        f1 = tmp_path / "config.json"
        f1.write_text("{}")
        f2 = tmp_path / "data.csv"
        f2.write_text("a,b,c")

        layer = tmp_path / "layer"
        layer.mkdir()
        build_app_layer(
            app_dir,
            _make_plan(),
            layer,
            verbose=False,
            include_paths=[f1, f2],
        )
        assert (layer / "app" / "config.json").exists()
        assert (layer / "app" / "data.csv").exists()

    def test_include_no_paths(self, tmp_path: Path) -> None:
        app_dir = tmp_path / "app"
        app_dir.mkdir()
        (app_dir / "main.py").write_text("print('hello')")

        layer = tmp_path / "layer"
        layer.mkdir()
        build_app_layer(
            app_dir,
            _make_plan(),
            layer,
            verbose=False,
            include_paths=None,
        )
        assert (layer / "app" / "main.py").read_text() == "print('hello')"

    def test_include_overwrites_existing(self, tmp_path: Path) -> None:
        app_dir = tmp_path / "app"
        app_dir.mkdir()
        (app_dir / "main.py").write_text("print('hello')")

        data_dir = tmp_path / "data"
        data_dir.mkdir()
        (data_dir / "file.txt").write_text("new content")

        # Pre-create the target directory to test overwrite
        layer = tmp_path / "layer"
        layer.mkdir()
        old_dir = layer / "app" / "data"
        old_dir.mkdir(parents=True)
        (old_dir / "file.txt").write_text("old content")

        build_app_layer(
            app_dir,
            _make_plan(),
            layer,
            verbose=False,
            include_paths=[data_dir],
        )
        assert (layer / "app" / "data" / "file.txt").read_text() == "new content"

    def test_include_symlink(self, tmp_path: Path) -> None:
        app_dir = tmp_path / "app"
        app_dir.mkdir()
        (app_dir / "main.py").write_text("print('hello')")

        real_file = tmp_path / "real.txt"
        real_file.write_text("real content")
        link_file = tmp_path / "link.txt"
        link_file.symlink_to(real_file)

        layer = tmp_path / "layer"
        layer.mkdir()
        build_app_layer(
            app_dir,
            _make_plan(),
            layer,
            verbose=False,
            include_paths=[link_file],
        )
        assert (layer / "app" / "link.txt").read_text() == "real content"
