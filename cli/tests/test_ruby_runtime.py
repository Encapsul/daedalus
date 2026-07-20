"""Unit tests for Ruby runtime detection."""

from unittest.mock import patch

import pytest

from xbin.runtimes.ruby import RubyRuntime


@pytest.fixture
def ruby_runtime():
    return RubyRuntime()


class TestRubyBundler:
    def test_detect_with_gemfile(self, ruby_runtime, tmp_path):
        (tmp_path / "Gemfile").write_text(
            "source 'https://rubygems.org'\ngem 'sinatra'"
        )
        (tmp_path / "main.rb").write_text("require 'sinatra'")
        with patch("xbin.runtimes.ruby.shutil.which", return_value="/usr/bin/ruby"):
            plan = ruby_runtime.detect(tmp_path)
        assert plan is not None
        assert plan.runtime == "ruby"

    def test_no_detect_without_gemfile_or_rb(self, ruby_runtime, tmp_path):
        (tmp_path / "README.md").write_text("hello")
        plan = ruby_runtime.detect(tmp_path)
        assert plan is None

    def test_detect_no_ruby_on_path(self, ruby_runtime, tmp_path):
        (tmp_path / "Gemfile").write_text("source 'https://rubygems.org'")
        with (
            patch("xbin.runtimes.ruby.shutil.which", return_value=None),
            pytest.raises(ValueError, match="no ruby on PATH"),
        ):
            ruby_runtime.detect(tmp_path)

    def test_entry_point_main_rb(self, ruby_runtime, tmp_path):
        (tmp_path / "Gemfile").write_text("source 'https://rubygems.org'")
        (tmp_path / "main.rb").write_text("puts 'hi'")
        with patch("xbin.runtimes.ruby.shutil.which", return_value="/usr/bin/ruby"):
            plan = ruby_runtime.detect(tmp_path)
        assert "/app/main.rb" in plan.entrypoint

    def test_entry_point_server_rb(self, ruby_runtime, tmp_path):
        (tmp_path / "Gemfile").write_text("source 'https://rubygems.org'")
        (tmp_path / "server.rb").write_text("require 'webrick'")
        with patch("xbin.runtimes.ruby.shutil.which", return_value="/usr/bin/ruby"):
            plan = ruby_runtime.detect(tmp_path)
        assert "/app/server.rb" in plan.entrypoint

    def test_entry_point_config_ru(self, ruby_runtime, tmp_path):
        (tmp_path / "Gemfile").write_text("source 'https://rubygems.org'")
        (tmp_path / "config.ru").write_text("run MyApp")
        with patch("xbin.runtimes.ruby.shutil.which", return_value="/usr/bin/ruby"):
            plan = ruby_runtime.detect(tmp_path)
        assert "/app/config.ru" in plan.entrypoint

    def test_detect_with_vendor_bundle(self, ruby_runtime, tmp_path):
        (tmp_path / "Gemfile").write_text("source 'https://rubygems.org'")
        (tmp_path / "main.rb").write_text("puts 'hi'")
        vb = tmp_path / "vendor" / "bundle"
        vb.mkdir(parents=True)
        with patch("xbin.runtimes.ruby.shutil.which", return_value="/usr/bin/ruby"):
            plan = ruby_runtime.detect(tmp_path)
        assert plan is not None
        assert plan.env.get("GEM_PATH") is not None


class TestRubySingleFile:
    def test_detect_single_rb(self, ruby_runtime, tmp_path):
        (tmp_path / "hello.rb").write_text("puts 'hi'")
        with patch("xbin.runtimes.ruby.shutil.which", return_value="/usr/bin/ruby"):
            plan = ruby_runtime.detect(tmp_path)
        assert plan is not None
        assert plan.runtime == "ruby"
        assert "/app/hello.rb" in plan.entrypoint

    def test_no_detect_multiple_rb(self, ruby_runtime, tmp_path):
        (tmp_path / "a.rb").write_text("puts 'a'")
        (tmp_path / "b.rb").write_text("puts 'b'")
        plan = ruby_runtime.detect(tmp_path)
        assert plan is None

    def test_no_detect_test_rb(self, ruby_runtime, tmp_path):
        (tmp_path / "test_something.rb").write_text("puts 'test'")
        plan = ruby_runtime.detect(tmp_path)
        assert plan is None


class TestRubyMisc:
    def test_supports_cross_false(self, ruby_runtime):
        assert ruby_runtime.supports_cross() is False
