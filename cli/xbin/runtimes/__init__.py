"""Runtime detection, registry, and shared data structures.

Each runtime (Python, Node, Deno, Java, Ruby, .NET, binary) lives in its own
file and implements the ``Runtime`` base class.  Detection order is fixed by
the registry: Python > Deno > Node > Binary.
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from pathlib import Path

# ---------------------------------------------------------------------------
# RuntimePlan — the resolved execution plan passed through the build pipeline
# ---------------------------------------------------------------------------


@dataclass
class RuntimePlan:
    """Resolved execution plan for an app.

    All ``*_host`` paths are absolute paths on the build machine.
    Entrypoint paths are relative to the rootfs (start with ``/``).
    """

    runtime: str  # "python" | "deno" | "node" | "java" | "ruby" | "dotnet" | "binary"
    interpreter_host: Path | None  # runtime binary to embed (None for native)
    entrypoint: list[str]  # argv relative to rootfs
    cwd: str = "/app"
    env: dict[str, str] = field(default_factory=dict)
    extra_dirs_host: list[Path] = field(default_factory=list)
    site_packages: list[tuple[Path, str]] = field(default_factory=list)


# ---------------------------------------------------------------------------
# Runtime base class
# ---------------------------------------------------------------------------


class Runtime(ABC):
    """Base class that every runtime must implement."""

    name: str  # e.g. "python", "node"

    @abstractmethod
    def detect(self, app_dir: Path) -> RuntimePlan | None:
        """Return a RuntimePlan if this runtime matches *app_dir*, else None."""

    def supports_cross(self) -> bool:
        """Whether cross-compilation is supported for this runtime."""
        return False


# ---------------------------------------------------------------------------
# Registry
# ---------------------------------------------------------------------------

_RUNTIME_REGISTRY: list[Runtime] = []


def register(rt: Runtime) -> None:
    """Register a runtime instance.  Order of registration = detection priority."""
    _RUNTIME_REGISTRY.append(rt)


def detect_runtime(app_dir: Path) -> RuntimePlan:
    """Try every registered runtime in order.  Raises ValueError on failure."""
    for rt in _RUNTIME_REGISTRY:
        plan = rt.detect(app_dir)
        if plan is not None:
            return plan
    raise ValueError(
        "could not detect runtime: no app.py/main.py, no deno.json/package.json, "
        "no pom.xml/build.gradle, no Gemfile, no *.csproj, no single ELF binary. "
        "Use a manifest (xbin.toml) to declare entrypoint."
    )


def get_runtime(name: str) -> Runtime:
    """Look up a runtime by name.  Raises KeyError if not found."""
    for rt in _RUNTIME_REGISTRY:
        if rt.name == name:
            return rt
    raise KeyError(name)


def _register_builtins() -> None:
    """Register the four built-in runtimes.  Called at module load time."""
    from .binary import BinaryRuntime
    from .deno import DenoRuntime
    from .node import NodeRuntime
    from .python import PythonRuntime

    register(PythonRuntime())
    register(DenoRuntime())
    register(NodeRuntime())
    register(BinaryRuntime())


_register_builtins()
