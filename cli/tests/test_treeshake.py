"""Tests for tree-shaking (unused node_modules pruning)."""

from __future__ import annotations

from pathlib import Path

from xbin.treeshake import (
    detect_used_packages,
    prune_node_modules,
)


def _make_node_app(app_dir: Path, imports: list[str], deps: dict[str, str]) -> None:
    """Create a minimal Node.js app with package.json and source files."""
    app_dir.mkdir(parents=True, exist_ok=True)

    import json

    pkg = {"name": "test-app", "dependencies": deps}
    (app_dir / "package.json").write_text(json.dumps(pkg))

    # Create source files with imports
    for i, imp in enumerate(imports):
        (app_dir / f"src{i}.js").write_text(f'const x = require("{imp}");\n')


def _make_nm(app_dir: Path, packages: list[str]) -> None:
    """Create fake node_modules directories."""
    nm = app_dir / "node_modules"
    nm.mkdir(parents=True, exist_ok=True)
    for pkg in packages:
        if pkg.startswith("@"):
            scope, name = pkg.split("/", 1)
            scope_dir = nm / scope
            scope_dir.mkdir(parents=True, exist_ok=True)
            (scope_dir / name).mkdir(parents=True, exist_ok=True)
        else:
            (nm / pkg).mkdir(parents=True, exist_ok=True)


class TestDetectUsedPackages:
    def test_basic_require(self, tmp_path: Path) -> None:
        _make_node_app(tmp_path, ["express", "lodash"], {"express": "^4", "lodash": "^4"})
        _make_nm(tmp_path, ["express", "lodash", "unused-pkg"])
        used = detect_used_packages(tmp_path)
        assert "express" in used
        assert "lodash" in used
        assert "unused-pkg" not in used

    def test_import_statement(self, tmp_path: Path) -> None:
        app_dir = tmp_path / "app"
        app_dir.mkdir()
        import json
        (app_dir / "package.json").write_text(
            json.dumps({"name": "t", "dependencies": {"react": "^18", "vue": "^3"}})
        )
        (app_dir / "index.jsx").write_text('import React from "react";\n')
        used = detect_used_packages(app_dir)
        assert "react" in used
        assert "vue" not in used

    def test_scoped_package(self, tmp_path: Path) -> None:
        app_dir = tmp_path / "app"
        app_dir.mkdir()
        import json
        (app_dir / "package.json").write_text(
            json.dumps(
                {
                    "name": "t",
                    "dependencies": {
                        "@angular/core": "^16",
                        "unused": "^1",
                    },
                }
            )
        )
        (app_dir / "main.js").write_text('require("@angular/core");\n')
        used = detect_used_packages(app_dir)
        assert "@angular/core" in used
        assert "unused" not in used

    def test_relative_imports_ignored(self, tmp_path: Path) -> None:
        _make_node_app(tmp_path, ["./utils", "../helper", "express"], {"express": "^4"})
        _make_nm(tmp_path, ["express"])
        used = detect_used_packages(tmp_path)
        assert used == {"express"}

    def test_no_package_json(self, tmp_path: Path) -> None:
        (tmp_path / "app").mkdir()
        used = detect_used_packages(tmp_path / "app")
        assert used == set()

    def test_skips_non_js_files(self, tmp_path: Path) -> None:
        app_dir = tmp_path / "app"
        app_dir.mkdir()
        import json
        (app_dir / "package.json").write_text(
            json.dumps({"name": "t", "dependencies": {"fs-extra": "^10"}})
        )
        (app_dir / "README.md").write_text('require("fs-extra")\n')
        used = detect_used_packages(app_dir)
        assert "fs-extra" not in used


class TestPruneNodeModules:
    def test_removes_unused(self, tmp_path: Path) -> None:
        _make_node_app(tmp_path, ["express"], {"express": "^4", "unused-pkg": "^1"})
        _make_nm(tmp_path, ["express", "unused-pkg"])
        removed = prune_node_modules(tmp_path, verbose=False)
        assert removed == 1
        assert (tmp_path / "node_modules" / "express").is_dir()
        assert not (tmp_path / "node_modules" / "unused-pkg").exists()

    def test_keeps_all_used(self, tmp_path: Path) -> None:
        _make_node_app(
            tmp_path,
            ["express", "lodash"],
            {"express": "^4", "lodash": "^4"},
        )
        _make_nm(tmp_path, ["express", "lodash"])
        removed = prune_node_modules(tmp_path, verbose=False)
        assert removed == 0
        assert (tmp_path / "node_modules" / "express").is_dir()
        assert (tmp_path / "node_modules" / "lodash").is_dir()

    def test_no_node_modules(self, tmp_path: Path) -> None:
        _make_node_app(tmp_path, ["express"], {"express": "^4"})
        removed = prune_node_modules(tmp_path, verbose=False)
        assert removed == 0

    def test_removes_scoped_packages(self, tmp_path: Path) -> None:
        app_dir = tmp_path / "app"
        app_dir.mkdir()
        import json
        (app_dir / "package.json").write_text(
            json.dumps(
                {
                    "name": "t",
                    "dependencies": {
                        "@angular/core": "^16",
                        "unused-scope": "^1",
                    },
                }
            )
        )
        (app_dir / "main.js").write_text('require("@angular/core");\n')

        nm = app_dir / "node_modules"
        (nm / "@angular" / "core").mkdir(parents=True)
        (nm / "@angular" / "router").mkdir(parents=True)
        (nm / "unused-scope").mkdir(parents=True)

        removed = prune_node_modules(app_dir, verbose=False)
        assert removed == 2
        assert (nm / "@angular" / "core").is_dir()
        assert not (nm / "@angular" / "router").exists()
        assert not (nm / "unused-scope").exists()
