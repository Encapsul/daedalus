"""xbin health checks: startup/readiness/liveness probes.

Provides a lightweight HTTP health endpoint that the launcher starts
in a background thread. Apps can mark themselves as ready via the
XBIN_HEALTH_PORT environment variable.

Environment variables set by the launcher:
  XBIN_HEALTH_PORT — port for the health endpoint (default: 8081)

Endpoints:
  GET /healthz — always 200 OK (liveness)
  GET /readyz  — 200 when app is ready, 503 otherwise
  GET /status  — JSON with uptime, version, status
"""

from __future__ import annotations

import json
import os
import threading
import time
from http.server import BaseHTTPRequestHandler, HTTPServer
from typing import Any


class HealthState:
    """Shared state for health check endpoints."""

    def __init__(self) -> None:
        self._ready = False
        self._started_at = time.time()
        self._version = os.environ.get("XBIN_VERSION", "unknown")
        self._extra: dict[str, Any] = {}

    def mark_ready(self) -> None:
        self._ready = True

    def mark_not_ready(self) -> None:
        self._ready = False

    @property
    def is_ready(self) -> bool:
        return self._ready

    @property
    def uptime(self) -> float:
        return time.time() - self._started_at

    def set_version(self, version: str) -> None:
        self._version = version

    def set_extra(self, key: str, value: Any) -> None:
        self._extra[key] = value

    def to_dict(self) -> dict[str, Any]:
        return {
            "status": "ready" if self._ready else "not_ready",
            "uptime_seconds": round(self.uptime, 2),
            "version": self._version,
            **self._extra,
        }


_health_state = HealthState()


def get_health_state() -> HealthState:
    """Return the global health state singleton."""
    return _health_state


class HealthHandler(BaseHTTPRequestHandler):
    """HTTP handler for health endpoints."""

    def do_GET(self) -> None:
        if self.path == "/healthz":
            self._respond(200, "OK")
        elif self.path == "/readyz":
            if _health_state.is_ready:
                self._respond(200, "Ready")
            else:
                self._respond(503, "Not Ready")
        elif self.path == "/status":
            body = json.dumps(_health_state.to_dict(), indent=2)
            self._respond_json(200, body)
        else:
            self._respond(404, "Not Found")

    def _respond(self, code: int, message: str) -> None:
        self.send_response(code)
        self.send_header("Content-Type", "text/plain")
        self.end_headers()
        self.wfile.write(message.encode())

    def _respond_json(self, code: int, body: str) -> None:
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(body.encode())

    def log_message(self, format: str, *args: Any) -> None:
        pass


def start_health_server(port: int | None = None) -> HTTPServer | None:
    """Start a health check HTTP server in a background thread.

    Returns the server instance, or None if port is 0 or disabled.
    """
    if port is None:
        port = int(os.environ.get("XBIN_HEALTH_PORT", "0"))
    if port == 0:
        return None

    server = HTTPServer(("0.0.0.0", port), HealthHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server


def stop_health_server(server: HTTPServer | None) -> None:
    """Stop the health check server."""
    if server is not None:
        server.shutdown()


def mark_ready() -> None:
    """Convenience: mark the app as ready."""
    _health_state.mark_ready()


def mark_not_ready() -> None:
    """Convenience: mark the app as not ready."""
    _health_state.mark_not_ready()
