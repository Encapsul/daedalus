"""Application runtime detection + entrypoint resolution.

Re-exports from ``xbin.runtimes`` for backward compatibility.
"""

from __future__ import annotations

from ..runtimes import RuntimePlan
from ..runtimes import detect_runtime as detect

__all__ = ["RuntimePlan", "detect"]
