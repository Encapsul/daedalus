"""Unit tests for Node.js runtime detection."""

from pathlib import Path
from unittest.mock import patch

import pytest

from xbin.runtimes.node import NodeRuntime


@pytest.fixture
def node_runtime():
    return NodeRuntime()


class TestNodeRuntime:
    def test_detect_with_package_json(self, node_runtime, tmp_path):
        (tmp_path / "package.json").write_text('{"name": "test", "main": "index.js"}')
        (tmp_path / "index.js").write_text("console.log('hello')")
        with patch("xbin.runtimes.node.shutil.which", return_value="/usr/bin/node"):
            plan = node_runtime.detect(tmp_path)
        assert plan is not None
        assert plan.runtime == "node"
        assert plan.interpreter_host == Path("/usr/bin/node")

    def test_no_detect_without_package_json(self, node_runtime, tmp_path):
        (tmp_path / "index.js").write_text("console.log('hello')")
        plan = node_runtime.detect(tmp_path)
        assert plan is None

    def test_detect_no_node_on_path(self, node_runtime, tmp_path):
        (tmp_path / "package.json").write_text('{"name": "test"}')
        with (
            patch("xbin.runtimes.node.shutil.which", return_value=None),
            pytest.raises(ValueError, match="no node on PATH"),
        ):
            node_runtime.detect(tmp_path)

    def test_entry_point_from_package_json_main(self, node_runtime, tmp_path):
        (tmp_path / "package.json").write_text('{"main": "server.js"}')
        (tmp_path / "server.js").write_text("require('http')")
        with patch("xbin.runtimes.node.shutil.which", return_value="/usr/bin/node"):
            plan = node_runtime.detect(tmp_path)
        assert "/app/server.js" in plan.entrypoint

    def test_entry_point_fallback_index_js(self, node_runtime, tmp_path):
        (tmp_path / "package.json").write_text('{"name": "test"}')
        (tmp_path / "index.js").write_text("console.log('hi')")
        with patch("xbin.runtimes.node.shutil.which", return_value="/usr/bin/node"):
            plan = node_runtime.detect(tmp_path)
        assert "/app/index.js" in plan.entrypoint

    def test_entry_point_fallback_server_js(self, node_runtime, tmp_path):
        (tmp_path / "package.json").write_text('{"name": "test"}')
        (tmp_path / "server.js").write_text("require('http')")
        with patch("xbin.runtimes.node.shutil.which", return_value="/usr/bin/node"):
            plan = node_runtime.detect(tmp_path)
        assert "/app/server.js" in plan.entrypoint

    def test_detect_with_node_modules(self, node_runtime, tmp_path):
        (tmp_path / "package.json").write_text('{"name": "test"}')
        (tmp_path / "index.js").write_text("console.log('hi')")
        (tmp_path / "node_modules").mkdir()
        with patch("xbin.runtimes.node.shutil.which", return_value="/usr/bin/node"):
            plan = node_runtime.detect(tmp_path)
        assert plan is not None
        assert len(plan.site_packages) == 1
        assert plan.env.get("NODE_PATH") is not None

    def test_supports_cross_false(self, node_runtime):
        assert node_runtime.supports_cross() is False
