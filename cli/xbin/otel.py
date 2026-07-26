"""xbin OpenTelemetry: auto-instrumentation hooks and environment setup.

Sets up standard OTel environment variables so apps can export traces,
metrics, and logs without configuration.

Environment variables set by the launcher:
  OTEL_SERVICE_NAME        — name of the service (from xbin metadata)
  OTEL_RESOURCE_ATTRIBUTES — service.name, service.version, deployment.mode
  OTEL_EXPORTER_OTLP_ENDPOINT — OTLP endpoint (if --otel-endpoint is set)
  OTEL_EXPORTER_OTLP_PROTOCOL — "grpc" or "http/protobuf"
  OTEL_METRICS_EXPORTER   — "otlp", "prometheus", or "none"
  OTEL_LOGS_EXPORTER      — "otlp", "console", or "none"
  OTEL_TRACES_EXPORTER    — "otlp", "console", or "none"
  OTEL_PYTHON_AUTO_INSTRUMENTATION_ENABLED — "true" (for Python auto-instrument)
"""

from __future__ import annotations

import os
from typing import Any


def build_otel_env(
    service_name: str,
    version: str = "",
    endpoint: str = "",
    protocol: str = "grpc",
    traces_exporter: str = "otlp",
    metrics_exporter: str = "otlp",
    logs_exporter: str = "none",
) -> dict[str, str]:
    """Build OTel environment variables for the launcher to inject.

    Args:
        service_name: name of the service (e.g. "myapp")
        version: service version string
        endpoint: OTLP collector endpoint (e.g. "http://localhost:4317")
        protocol: "grpc" or "http/protobuf"
        traces_exporter: "otlp", "console", or "none"
        metrics_exporter: "otlp", "prometheus", or "none"
        logs_exporter: "otlp", "console", or "none"

    Returns:
        Dict of environment variables to inject.
    """
    env: dict[str, str] = {
        "OTEL_SERVICE_NAME": service_name,
        "OTEL_TRACES_EXPORTER": traces_exporter,
        "OTEL_METRICS_EXPORTER": metrics_exporter,
        "OTEL_LOGS_EXPORTER": logs_exporter,
    }

    resource_attrs = f"service.name={service_name}"
    if version:
        resource_attrs += f",service.version={version}"
    resource_attrs += ",deployment.mode=server"
    env["OTEL_RESOURCE_ATTRIBUTES"] = resource_attrs

    if endpoint:
        env["OTEL_EXPORTER_OTLP_ENDPOINT"] = endpoint
        env["OTEL_EXPORTER_OTLP_PROTOCOL"] = protocol

    # Python auto-instrumentation hook
    if traces_exporter != "none" or metrics_exporter != "none":
        env["OTEL_PYTHON_AUTO_INSTRUMENTATION_ENABLED"] = "true"

    return env


def get_otel_config() -> dict[str, Any]:
    """Read current OTel configuration from environment.

    Returns a dict with all OTel-related env vars, useful for introspection.
    """
    otel_vars = [
        "OTEL_SERVICE_NAME",
        "OTEL_RESOURCE_ATTRIBUTES",
        "OTEL_EXPORTER_OTLP_ENDPOINT",
        "OTEL_EXPORTER_OTLP_PROTOCOL",
        "OTEL_TRACES_EXPORTER",
        "OTEL_METRICS_EXPORTER",
        "OTEL_LOGS_EXPORTER",
        "OTEL_PYTHON_AUTO_INSTRUMENTATION_ENABLED",
    ]
    config: dict[str, Any] = {}
    for var in otel_vars:
        val = os.environ.get(var)
        if val is not None:
            config[var] = val
    return config


def format_resource_attributes(attrs_str: str) -> dict[str, str]:
    """Parse OTEL_RESOURCE_ATTRIBUTES string into a dict.

    Format: "key1=value1,key2=value2"
    """
    result: dict[str, str] = {}
    if not attrs_str:
        return result
    for pair in attrs_str.split(","):
        if "=" in pair:
            key, _, value = pair.partition("=")
            result[key.strip()] = value.strip()
    return result
