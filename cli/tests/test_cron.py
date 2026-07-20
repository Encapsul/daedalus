"""Tests for cron/scheduled tasks feature."""

from __future__ import annotations

import json
import time

from xbin.cron import (
    CronScheduler,
    Task,
    _parse_interval,
    _parse_schedule,
    build_cron_env,
    get_scheduler,
)


class TestParseSchedule:
    def test_every_5m(self) -> None:
        assert _parse_schedule("@every 5m") == 300

    def test_every_30s(self) -> None:
        assert _parse_schedule("@every 30s") == 30

    def test_every_2h(self) -> None:
        assert _parse_schedule("@every 2h") == 7200

    def test_hourly(self) -> None:
        assert _parse_schedule("@hourly") == 3600

    def test_daily(self) -> None:
        assert _parse_schedule("@daily") == 86400

    def test_weekly(self) -> None:
        assert _parse_schedule("@weekly") == 604800

    def test_cron_every_5_min(self) -> None:
        assert _parse_schedule("*/5 * * * *") == 300

    def test_cron_every_15_min(self) -> None:
        assert _parse_schedule("*/15 * * * *") == 900

    def test_fixed_time(self) -> None:
        assert _parse_schedule("0 12 * * *") == 60

    def test_parse_interval_seconds(self) -> None:
        assert _parse_interval("30s") == 30

    def test_parse_interval_minutes(self) -> None:
        assert _parse_interval("5m") == 300

    def test_parse_interval_hours(self) -> None:
        assert _parse_interval("2h") == 7200


class TestCronScheduler:
    def test_add_task(self) -> None:
        sched = CronScheduler()
        called = []
        sched.add_task(Task("test", "@every 1h", lambda: called.append(1)))
        assert sched.task_count == 1

    def test_remove_task(self) -> None:
        sched = CronScheduler()
        sched.add_task(Task("test", "@every 1h", lambda: None))
        assert sched.remove_task("test")
        assert sched.task_count == 0
        assert not sched.remove_task("nonexistent")

    def test_tick_runs_due_task(self) -> None:
        sched = CronScheduler()
        called = []
        task = Task("test", "@every 1s", lambda: called.append(1))
        sched.add_task(task)
        task.next_run = 0  # force due after add_task
        sched.tick()
        assert called == [1]

    def test_tick_skips_disabled_task(self) -> None:
        sched = CronScheduler()
        called = []
        task = Task("test", "@every 1s", lambda: called.append(1))
        task.enabled = False
        sched.add_task(task)
        task.next_run = 0  # force due after add_task
        sched.tick()
        assert called == []

    def test_tick_catches_errors(self) -> None:
        sched = CronScheduler()
        task = Task("test", "@every 1s", lambda: 1 / 0)
        sched.add_task(task)
        task.next_run = 0  # force due after add_task
        sched.tick()
        assert task.error is not None
        assert "division by zero" in task.error

    def test_start_stop(self) -> None:
        sched = CronScheduler()
        sched.start()
        assert sched.is_running
        sched.stop()
        assert not sched.is_running

    def test_get_task_status(self) -> None:
        sched = CronScheduler()
        sched.add_task(Task("cleanup", "@every 5m", lambda: None))
        status = sched.get_task_status()
        assert len(status) == 1
        assert status[0]["name"] == "cleanup"
        assert status[0]["schedule"] == "@every 5m"
        assert status[0]["enabled"] is True

    def test_singleton(self) -> None:
        s1 = get_scheduler()
        s2 = get_scheduler()
        assert s1 is s2


class TestBuildCronEnv:
    def test_with_tasks(self) -> None:
        tasks = [{"name": "cleanup", "schedule": "*/5 * * * *"}]
        env = build_cron_env(tasks)
        assert env["XBIN_CRON_ENABLED"] == "true"
        parsed = json.loads(env["XBIN_CRON_TASKS"])
        assert parsed[0]["name"] == "cleanup"

    def test_empty_tasks(self) -> None:
        env = build_cron_env([])
        assert env == {}
