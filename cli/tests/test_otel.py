"""Tests for OpenTelemetry feature."""

from __future__ import annotations

import os

from xbin.otel import (
    build_otel_env,
    format_resource_attributes,
    get_otel_config,
)


class TestBuildOtelEnv:
    def test_basic(self) -> None:
        env = build_otel_env("myapp")
        assert env["OTEL_SERVICE_NAME"] == "myapp"
        assert "service.name=myapp" in env["OTEL_RESOURCE_ATTRIBUTES"]
        assert env["OTEL_TRACES_EXPORTER"] == "otlp"

    def test_with_version(self) -> None:
        env = build_otel_env("myapp", version="1.2.3")
        assert "service.version=1.2.3" in env["OTEL_RESOURCE_ATTRIBUTES"]

    def test_with_endpoint(self) -> None:
        env = build_otel_env("myapp", endpoint="http://localhost:4317")
        assert env["OTEL_EXPORTER_OTLP_ENDPOINT"] == "http://localhost:4317"
        assert env["OTEL_EXPORTER_OTLP_PROTOCOL"] == "grpc"

    def test_http_protocol(self) -> None:
        env = build_otel_env("myapp", endpoint="http://localhost:4318", protocol="http/protobuf")
        assert env["OTEL_EXPORTER_OTLP_PROTOCOL"] == "http/protobuf"

    def test_no_endpoint_no_otlp_vars(self) -> None:
        env = build_otel_env("myapp")
        assert "OTEL_EXPORTER_OTLP_ENDPOINT" not in env

    def test_custom_exporters(self) -> None:
        env = build_otel_env(
            "myapp",
            traces_exporter="console",
            metrics_exporter="prometheus",
            logs_exporter="console",
        )
        assert env["OTEL_TRACES_EXPORTER"] == "console"
        assert env["OTEL_METRICS_EXPORTER"] == "prometheus"
        assert env["OTEL_LOGS_EXPORTER"] == "console"

    def test_auto_instrument_enabled(self) -> None:
        env = build_otel_env("myapp", traces_exporter="otlp")
        assert env.get("OTEL_PYTHON_AUTO_INSTRUMENTATION_ENABLED") == "true"

    def test_auto_instrument_disabled_when_no_exporters(self) -> None:
        env = build_otel_env(
            "myapp", traces_exporter="none", metrics_exporter="none"
        )
        assert "OTEL_PYTHON_AUTO_INSTRUMENTATION_ENABLED" not in env

    def test_deployment_mode(self) -> None:
        env = build_otel_env("myapp")
        assert "deployment.mode=server" in env["OTEL_RESOURCE_ATTRIBUTES"]


class TestFormatResourceAttributes:
    def test_basic(self) -> None:
        result = format_resource_attributes("a=1,b=2")
        assert result == {"a": "1", "b": "2"}

    def test_empty(self) -> None:
        assert format_resource_attributes("") == {}

    def test_spaces(self) -> None:
        result = format_resource_attributes("a = 1 , b = 2")
        assert result == {"a": "1", "b": "2"}

    def test_single(self) -> None:
        result = format_resource_attributes("key=value")
        assert result == {"key": "value"}


class TestGetOtelConfig:
    def test_reads_env(self, monkeypatch) -> None:
        monkeypatch.setenv("OTEL_SERVICE_NAME", "test")
        monkeypatch.setenv("OTEL_TRACES_EXPORTER", "otlp")
        config = get_otel_config()
        assert config["OTEL_SERVICE_NAME"] == "test"
        assert config["OTEL_TRACES_EXPORTER"] == "otlp"

    def test_empty_when_no_env(self, monkeypatch) -> None:
        for var in [
            "OTEL_SERVICE_NAME",
            "OTEL_RESOURCE_ATTRIBUTES",
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            "OTEL_EXPORTER_OTLP_PROTOCOL",
            "OTEL_TRACES_EXPORTER",
            "OTEL_METRICS_EXPORTER",
            "OTEL_LOGS_EXPORTER",
            "OTEL_PYTHON_AUTO_INSTRUMENTATION_ENABLED",
        ]:
            monkeypatch.delenv(var, raising=False)
        config = get_otel_config()
        assert len(config) == 0
