"""Tests for --minify feature."""

from __future__ import annotations

from pathlib import Path

from xbin.minify import _minify_css_simple, minify_app_dir


class TestCSSMinification:
    def test_strip_comments(self) -> None:
        css = "/* comment */ .a { color: red; }"
        result = _minify_css_simple(css)
        assert "/*" not in result
        assert ".a{color:red;}" == result

    def test_collapse_whitespace(self) -> None:
        css = ".a  {\n  color:  red;\n  background: blue;\n}"
        result = _minify_css_simple(css)
        assert "\n" not in result
        assert "  " not in result
        assert ".a{color:red;background:blue;}" == result

    def test_empty(self) -> None:
        assert _minify_css_simple("") == ""
        assert _minify_css_simple("   ") == ""


class TestMinifyAppDir:
    def test_minifies_css_files(self, tmp_path: Path) -> None:
        css = "/* comment */\n.body {\n  color: red;\n  margin: 0;\n}\n"
        (tmp_path / "style.css").write_text(css)
        count = minify_app_dir(tmp_path, verbose=False)
        assert count == 1
        result = (tmp_path / "style.css").read_text()
        assert "/*" not in result
        assert "\n" not in result

    def test_skips_node_modules(self, tmp_path: Path) -> None:
        nm = tmp_path / "node_modules" / "pkg"
        nm.mkdir(parents=True)
        css = "/* comment */ .a { color: red; }"
        (nm / "style.css").write_text(css)
        count = minify_app_dir(tmp_path, verbose=False)
        assert count == 0

    def test_minifies_js_with_terser(self, tmp_path: Path) -> None:
        """Only runs if terser is installed."""
        import shutil
        if not shutil.which("terser"):
            return
        js = "function hello() {\n  var x = 1;\n  return x;\n}\n"
        (tmp_path / "app.js").write_text(js)
        count = minify_app_dir(tmp_path, verbose=False)
        assert count == 1
        result = (tmp_path / "app.js").read_text()
        assert len(result) < len(js)

    def test_no_files_to_minify(self, tmp_path: Path) -> None:
        (tmp_path / "readme.txt").write_text("hello")
        count = minify_app_dir(tmp_path, verbose=False)
        assert count == 0
