"""Unit tests for Hugo runtime detection."""

from unittest.mock import patch

import pytest

from xbin.runtimes.hugo import HugoRuntime


@pytest.fixture
def hugo_runtime():
    return HugoRuntime()


class TestHugoDetection:
    def test_detect_hugo_toml(self, hugo_runtime, tmp_path):
        (tmp_path / "hugo.toml").write_text('baseURL = "https://example.com"')
        plan = hugo_runtime.detect(tmp_path)
        assert plan is not None
        assert plan.runtime == "hugo"

    def test_detect_config_toml(self, hugo_runtime, tmp_path):
        (tmp_path / "config.toml").write_text(
            'baseURL = "https://example.com"\nlanguageCode = "en"'
        )
        plan = hugo_runtime.detect(tmp_path)
        assert plan is not None
        assert plan.runtime == "hugo"

    def test_detect_config_yaml(self, hugo_runtime, tmp_path):
        (tmp_path / "config.yaml").write_text(
            'baseURL: "https://example.com"\nlanguageCode: en'
        )
        plan = hugo_runtime.detect(tmp_path)
        assert plan is not None

    def test_no_detect_without_hugo_config(self, hugo_runtime, tmp_path):
        (tmp_path / "README.md").write_text("hello")
        plan = hugo_runtime.detect(tmp_path)
        assert plan is None

    def test_generic_config_not_hugo(self, hugo_runtime, tmp_path):
        (tmp_path / "config.toml").write_text("some_value = true")
        plan = hugo_runtime.detect(tmp_path)
        assert plan is None

    def test_detect_with_hugo_binary(self, hugo_runtime, tmp_path):
        (tmp_path / "hugo.toml").write_text('baseURL = "https://example.com"')
        with patch("xbin.runtimes.hugo.shutil.which") as mock_which:
            mock_which.side_effect = lambda cmd: {
                "hugo": "/usr/bin/hugo",
                "python3": "/usr/bin/python3",
            }.get(cmd)
            plan = hugo_runtime.detect(tmp_path)
        assert plan is not None
        assert "hugo" in plan.entrypoint[0]

    def test_hugo_builds_and_serves(self, hugo_runtime, tmp_path):
        (tmp_path / "hugo.toml").write_text('baseURL = "https://example.com"')
        with patch("xbin.runtimes.hugo.shutil.which") as mock_which:
            mock_which.side_effect = lambda cmd: {
                "hugo": "/usr/bin/hugo",
                "python3": "/usr/bin/python3",
            }.get(cmd)
            plan = hugo_runtime.detect(tmp_path)
        assert plan is not None
        assert "--minify" in plan.entrypoint

    def test_supports_cross_true(self, hugo_runtime):
        assert hugo_runtime.supports_cross() is True
