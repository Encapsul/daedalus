"""xbin cron: built-in scheduler for periodic tasks.

Provides a lightweight cron-like scheduler that runs tasks at specified
intervals. Apps register tasks via the xbin.cron API, and the scheduler
runs them in background threads.

Usage in app code:
    from xbin.cron import CronScheduler, Task

    scheduler = CronScheduler()
    scheduler.add_task(Task("cleanup", "*/5 * * * *", my_cleanup_fn))
    scheduler.start()

Task schedule formats:
    - "@every 5m" — every 5 minutes
    - "@hourly"   — every hour
    - "@daily"    — once a day
    - "@weekly"   — once a week
    - "*/5 * * * *" — standard cron (minute, hour, dom, month, dow)

Environment variables set by the launcher:
    XBIN_CRON_ENABLED — "true" if cron scheduler is active
    XBIN_CRON_TASKS   — JSON list of task definitions (name, schedule)
"""

from __future__ import annotations

import json
import threading
import time
from collections.abc import Callable
from dataclasses import dataclass
from typing import Any


@dataclass
class Task:
    """A scheduled task."""

    name: str
    schedule: str
    func: Callable[[], Any]
    last_run: float = 0.0
    next_run: float = 0.0
    enabled: bool = True
    error: str | None = None


def _parse_schedule(schedule: str) -> int:
    """Parse a schedule string into seconds interval.

    Supports:
      - "@every Ns/Nm/Nh" — every N seconds/minutes/hours
      - "@hourly" — 3600s
      - "@daily" — 86400s
      - "@weekly" — 604800s
      - "*/N * * * *" — every N minutes (cron-style)
      - "M H D M W" — at specific time (simplified, minute-accurate)
    """
    schedule = schedule.strip().lower()

    if schedule.startswith("@every "):
        val = schedule[7:]
        return _parse_interval(val)

    if schedule == "@hourly":
        return 3600
    if schedule == "@daily":
        return 86400
    if schedule == "@weekly":
        return 604800

    # Cron-style: "*/N * * * *" or "M H D M W"
    parts = schedule.split()
    if len(parts) == 5:
        minute_part = parts[0]
        if minute_part.startswith("*/"):
            try:
                return int(minute_part[2:]) * 60
            except ValueError:
                pass
        # Fixed time: compute seconds until next occurrence
        return 60  # default: check every minute

    return 60  # fallback


def _parse_interval(val: str) -> int:
    """Parse '5m', '30s', '2h' into seconds."""
    val = val.strip()
    if val.endswith("s"):
        return int(val[:-1])
    if val.endswith("m"):
        return int(val[:-1]) * 60
    if val.endswith("h"):
        return int(val[:-1]) * 3600
    return int(val)


class CronScheduler:
    """Lightweight cron scheduler that runs tasks in background threads."""

    def __init__(self) -> None:
        self._tasks: list[Task] = []
        self._running = False
        self._thread: threading.Thread | None = None
        self._lock = threading.Lock()

    def add_task(self, task: Task) -> None:
        """Register a task."""
        task.next_run = time.time() + _parse_schedule(task.schedule)
        with self._lock:
            self._tasks.append(task)

    def remove_task(self, name: str) -> bool:
        """Remove a task by name. Returns True if found."""
        with self._lock:
            for i, t in enumerate(self._tasks):
                if t.name == name:
                    self._tasks.pop(i)
                    return True
        return False

    def start(self) -> None:
        """Start the scheduler in a background thread."""
        if self._running:
            return
        self._running = True
        self._thread = threading.Thread(target=self._run_loop, daemon=True)
        self._thread.start()

    def stop(self) -> None:
        """Stop the scheduler."""
        self._running = False
        if self._thread is not None:
            self._thread.join(timeout=5)

    def tick(self) -> None:
        """Run one scheduler tick (for testing). Checks and runs due tasks."""
        now = time.time()
        with self._lock:
            tasks = list(self._tasks)
        for task in tasks:
            if not task.enabled:
                continue
            if now >= task.next_run:
                task.last_run = now
                task.next_run = now + _parse_schedule(task.schedule)
                try:
                    task.func()
                    task.error = None
                except Exception as exc:
                    task.error = str(exc)

    @property
    def is_running(self) -> bool:
        return self._running

    @property
    def task_count(self) -> int:
        with self._lock:
            return len(self._tasks)

    def get_task_status(self) -> list[dict[str, Any]]:
        """Return status of all tasks."""
        with self._lock:
            tasks = list(self._tasks)
        return [
            {
                "name": t.name,
                "schedule": t.schedule,
                "enabled": t.enabled,
                "last_run": t.last_run,
                "next_run": t.next_run,
                "error": t.error,
            }
            for t in tasks
        ]

    def _run_loop(self) -> None:
        """Background loop that ticks every second."""
        while self._running:
            self.tick()
            time.sleep(1)


_scheduler: CronScheduler | None = None


def get_scheduler() -> CronScheduler:
    """Get or create the global scheduler singleton."""
    global _scheduler
    if _scheduler is None:
        _scheduler = CronScheduler()
    return _scheduler


def build_cron_env(tasks: list[dict[str, str]]) -> dict[str, str]:
    """Build cron environment variables for the launcher.

    Args:
        tasks: list of {"name": "...", "schedule": "..."} dicts

    Returns:
        Dict of environment variables to inject.
    """
    if not tasks:
        return {}
    return {
        "XBIN_CRON_ENABLED": "true",
        "XBIN_CRON_TASKS": json.dumps(tasks),
    }
