"""Unit tests for Perl runtime detection."""

import tempfile
from pathlib import Path

import pytest

from xbin.runtimes.perl import PerlRuntime


@pytest.fixture
def perl_runtime():
    return PerlRuntime()


class TestPerlRuntime:
    def test_detect_with_makefile_pl(self, perl_runtime, tmp_path):
        makefile = tmp_path / "Makefile.PL"
        makefile.write_text('use ExtUtils::MakeMaker;\nWriteMakefile(NAME => "My::App");')
        app_pl = tmp_path / "app.pl"
        app_pl.write_text("#!/usr/bin/env perl\nprint 'hello\\n';")

        plan = perl_runtime.detect(tmp_path)
        assert plan is not None
        assert plan.runtime == "perl"

    def test_detect_with_cpanfile(self, perl_runtime, tmp_path):
        cpanfile = tmp_path / "cpanfile"
        cpanfile.write_text("requires 'Mojolicious', '0';")
        app_pl = tmp_path / "app.pl"
        app_pl.write_text("#!/usr/bin/env perl\nprint 'hello\\n';")

        plan = perl_runtime.detect(tmp_path)
        assert plan is not None
        assert plan.runtime == "perl"

    def test_no_detect_without_perl_files(self, perl_runtime, tmp_path):
        readme = tmp_path / "README.md"
        readme.write_text("# My App")

        plan = perl_runtime.detect(tmp_path)
        assert plan is None

    def test_supports_cross_false(self, perl_runtime):
        assert perl_runtime.supports_cross() is False

    def test_app_pl_entry(self, perl_runtime, tmp_path):
        makefile = tmp_path / "Makefile.PL"
        makefile.write_text("use ExtUtils::MakeMaker;")
        app_pl = tmp_path / "app.pl"
        app_pl.write_text("#!/usr/bin/env perl")

        plan = perl_runtime.detect(tmp_path)
        assert plan is not None
        assert "app.pl" in plan.entrypoint[1]

    def test_bin_app_entry(self, perl_runtime, tmp_path):
        makefile = tmp_path / "Makefile.PL"
        makefile.write_text("use ExtUtils::MakeMaker;")
        bin_dir = tmp_path / "bin"
        bin_dir.mkdir()
        app = bin_dir / "app"
        app.write_text("#!/usr/bin/env perl")

        plan = perl_runtime.detect(tmp_path)
        assert plan is not None
        assert "bin/app" in plan.entrypoint[1]
