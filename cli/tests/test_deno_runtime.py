"""Unit tests for Deno runtime detection."""

from pathlib import Path
from unittest.mock import patch

import pytest

from xbin.runtimes.deno import DenoRuntime


@pytest.fixture
def deno_runtime():
    return DenoRuntime()


class TestDenoRuntime:
    def test_detect_with_deno_json(self, deno_runtime, tmp_path):
        (tmp_path / "deno.json").write_text('{"tasks": {"start": "deno run main.ts"}}')
        (tmp_path / "main.ts").write_text("console.log('hello')")
        with patch("xbin.runtimes.deno.shutil.which", return_value="/usr/bin/deno"):
            plan = deno_runtime.detect(tmp_path)
        assert plan is not None
        assert plan.runtime == "deno"
        assert plan.interpreter_host == Path("/usr/bin/deno")

    def test_detect_with_deno_jsonc(self, deno_runtime, tmp_path):
        (tmp_path / "deno.jsonc").write_text('// comment\n{"tasks": {}}')
        (tmp_path / "main.ts").write_text("console.log('hello')")
        with patch("xbin.runtimes.deno.shutil.which", return_value="/usr/bin/deno"):
            plan = deno_runtime.detect(tmp_path)
        assert plan is not None

    def test_no_detect_without_deno_config(self, deno_runtime, tmp_path):
        (tmp_path / "main.ts").write_text("console.log('hello')")
        plan = deno_runtime.detect(tmp_path)
        assert plan is None

    def test_entry_point_from_tasks(self, deno_runtime, tmp_path):
        (tmp_path / "deno.json").write_text('{"tasks": {"start": "deno run app.ts"}}')
        (tmp_path / "app.ts").write_text("Deno.serve(() => new Response('hi'))")
        with patch("xbin.runtimes.deno.shutil.which", return_value="/usr/bin/deno"):
            plan = deno_runtime.detect(tmp_path)
        assert "/app/app.ts" in plan.entrypoint

    def test_entry_point_fallback_main_ts(self, deno_runtime, tmp_path):
        (tmp_path / "deno.json").write_text("{}")
        (tmp_path / "main.ts").write_text("console.log('hi')")
        with patch("xbin.runtimes.deno.shutil.which", return_value="/usr/bin/deno"):
            plan = deno_runtime.detect(tmp_path)
        assert "/app/main.ts" in plan.entrypoint

    def test_entry_point_fallback_mod_ts(self, deno_runtime, tmp_path):
        (tmp_path / "deno.json").write_text("{}")
        (tmp_path / "mod.ts").write_text("console.log('hi')")
        with patch("xbin.runtimes.deno.shutil.which", return_value="/usr/bin/deno"):
            plan = deno_runtime.detect(tmp_path)
        assert "/app/mod.ts" in plan.entrypoint

    def test_allow_all_flag(self, deno_runtime, tmp_path):
        (tmp_path / "deno.json").write_text("{}")
        (tmp_path / "main.ts").write_text("console.log('hi')")
        with patch("xbin.runtimes.deno.shutil.which", return_value="/usr/bin/deno"):
            plan = deno_runtime.detect(tmp_path)
        assert "--allow-all" in plan.entrypoint

    def test_supports_cross_false(self, deno_runtime):
        assert deno_runtime.supports_cross() is False
