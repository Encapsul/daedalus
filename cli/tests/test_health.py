"""Tests for health checks feature."""

from __future__ import annotations

import json
import os
import threading
import time
from http.client import HTTPConnection
from pathlib import Path

from xbin.health import (
    HealthState,
    get_health_state,
    mark_not_ready,
    mark_ready,
    start_health_server,
    stop_health_server,
)


class TestHealthState:
    def test_initial_state(self) -> None:
        state = HealthState()
        assert not state.is_ready
        assert state.uptime >= 0
        d = state.to_dict()
        assert d["status"] == "not_ready"

    def test_mark_ready(self) -> None:
        state = HealthState()
        state.mark_ready()
        assert state.is_ready
        assert state.to_dict()["status"] == "ready"

    def test_mark_not_ready(self) -> None:
        state = HealthState()
        state.mark_ready()
        state.mark_not_ready()
        assert not state.is_ready

    def test_uptime(self) -> None:
        state = HealthState()
        time.sleep(0.05)
        assert state.uptime >= 0.04

    def test_set_version(self) -> None:
        state = HealthState()
        state.set_version("1.2.3")
        assert state.to_dict()["version"] == "1.2.3"

    def test_set_extra(self) -> None:
        state = HealthState()
        state.set_extra("connections", 42)
        d = state.to_dict()
        assert d["connections"] == 42


class TestHealthServer:
    def test_liveness_endpoint(self) -> None:
        port = _free_port()
        server = start_health_server(port)
        try:
            conn = HTTPConnection("127.0.0.1", port, timeout=2)
            conn.request("GET", "/healthz")
            resp = conn.getresponse()
            assert resp.status == 200
            assert resp.read() == b"OK"
            conn.close()
        finally:
            stop_health_server(server)

    def test_readiness_not_ready(self) -> None:
        state = get_health_state()
        state.mark_not_ready()
        port = _free_port()
        server = start_health_server(port)
        try:
            conn = HTTPConnection("127.0.0.1", port, timeout=2)
            conn.request("GET", "/readyz")
            resp = conn.getresponse()
            assert resp.status == 503
            conn.close()
        finally:
            stop_health_server(server)

    def test_readiness_ready(self) -> None:
        state = get_health_state()
        state.mark_ready()
        port = _free_port()
        server = start_health_server(port)
        try:
            conn = HTTPConnection("127.0.0.1", port, timeout=2)
            conn.request("GET", "/readyz")
            resp = conn.getresponse()
            assert resp.status == 200
            conn.close()
        finally:
            state.mark_not_ready()
            stop_health_server(server)

    def test_status_json(self) -> None:
        port = _free_port()
        server = start_health_server(port)
        try:
            conn = HTTPConnection("127.0.0.1", port, timeout=2)
            conn.request("GET", "/status")
            resp = conn.getresponse()
            assert resp.status == 200
            data = json.loads(resp.read())
            assert "status" in data
            assert "uptime_seconds" in data
            conn.close()
        finally:
            stop_health_server(server)

    def test_404_for_unknown(self) -> None:
        port = _free_port()
        server = start_health_server(port)
        try:
            conn = HTTPConnection("127.0.0.1", port, timeout=2)
            conn.request("GET", "/unknown")
            resp = conn.getresponse()
            assert resp.status == 404
            conn.close()
        finally:
            stop_health_server(server)

    def test_disabled_when_port_zero(self) -> None:
        server = start_health_server(0)
        assert server is None


def _free_port() -> int:
    import socket
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]
