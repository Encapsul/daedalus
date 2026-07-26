"""Unit tests for .NET runtime detection."""

from unittest.mock import patch

import pytest

from xbin.runtimes.dotnet import DotnetRuntime


@pytest.fixture
def dotnet_runtime():
    return DotnetRuntime()


class TestDotnetDetection:
    def test_detect_csproj(self, dotnet_runtime, tmp_path):
        csproj = tmp_path / "MyApp.csproj"
        csproj.write_text(
            '<Project Sdk="Microsoft.NET.Sdk">'
            "<PropertyGroup><OutputType>Exe</OutputType></PropertyGroup>"
            "</Project>"
        )
        with patch("xbin.runtimes.dotnet.shutil.which", return_value="/usr/bin/dotnet"):
            plan = dotnet_runtime.detect(tmp_path)
        assert plan is not None
        assert plan.runtime == "dotnet"

    def test_no_detect_without_csproj(self, dotnet_runtime, tmp_path):
        (tmp_path / "Program.cs").write_text("Console.WriteLine('hi');")
        plan = dotnet_runtime.detect(tmp_path)
        assert plan is None

    def test_detect_no_dotnet_on_path(self, dotnet_runtime, tmp_path):
        (tmp_path / "MyApp.csproj").write_text(
            '<Project Sdk="Microsoft.NET.Sdk"></Project>'
        )
        with (
            patch("xbin.runtimes.dotnet.shutil.which", return_value=None),
            pytest.raises(ValueError, match="no dotnet on PATH"),
        ):
            dotnet_runtime.detect(tmp_path)


class TestDotnetPublished:
    def test_detect_with_publish_dir(self, dotnet_runtime, tmp_path):
        (tmp_path / "MyApp.csproj").write_text(
            "<Project><PropertyGroup><OutputType>Exe</OutputType></PropertyGroup></Project>"
        )
        pub = tmp_path / "publish"
        pub.mkdir()
        (pub / "MyApp.dll").write_bytes(b"fake dll")
        with patch("xbin.runtimes.dotnet.shutil.which", return_value="/usr/bin/dotnet"):
            plan = dotnet_runtime.detect(tmp_path)
        assert plan is not None
        assert "publish" in " ".join(plan.entrypoint)

    def test_library_raises(self, dotnet_runtime, tmp_path):
        csproj = tmp_path / "MyLib.csproj"
        csproj.write_text(
            "<Project><PropertyGroup><OutputType>Library</OutputType></PropertyGroup></Project>"
        )
        with (
            patch("xbin.runtimes.dotnet.shutil.which", return_value="/usr/bin/dotnet"),
            pytest.raises(ValueError, match="class library"),
        ):
            dotnet_runtime.detect(tmp_path)


class TestDotnetMisc:
    def test_supports_cross_false(self, dotnet_runtime):
        assert dotnet_runtime.supports_cross() is False
