"""Unit tests for runtime registry."""

import pytest

from xbin.runtimes import (
    _RUNTIME_REGISTRY,
    detect_runtime,
    get_runtime,
)


class TestRegistry:
    def test_registry_not_empty(self):
        assert len(_RUNTIME_REGISTRY) > 0

    def test_registry_contains_all_runtimes(self):
        names = [rt.name for rt in _RUNTIME_REGISTRY]
        assert "python" in names
        assert "node" in names
        assert "deno" in names
        assert "java" in names
        assert "ruby" in names
        assert "dotnet" in names
        assert "hugo" in names
        assert "go" in names
        assert "php" in names
        assert "perl" in names
        assert "binary" in names

    def test_get_runtime(self):
        rt = get_runtime("python")
        assert rt.name == "python"

    def test_get_runtime_not_found(self):
        with pytest.raises(KeyError):
            get_runtime("nonexistent")

    def test_detection_order_python_first(self, tmp_path):
        app_py = tmp_path / "app.py"
        app_py.write_text("print('hello')")
        package_json = tmp_path / "package.json"
        package_json.write_text('{"name": "test"}')

        plan = detect_runtime(tmp_path)
        assert plan.runtime == "python"
