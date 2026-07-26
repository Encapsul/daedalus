"""Tests for persistent storage module."""

from __future__ import annotations

import os
from pathlib import Path

from xbin.persistent import ensure_persist_dir, get_persist_dir, get_persist_env


class TestGetPersistDir:
    """Tests for get_persist_dir()."""

    def test_xdg_data_home(self, tmp_path: Path) -> None:
        os.environ["XDG_DATA_HOME"] = str(tmp_path)
        try:
            result = get_persist_dir("my-app")
            assert result == tmp_path / "xbin" / "my-app"
        finally:
            del os.environ["XDG_DATA_HOME"]

    def test_default_path(self, tmp_path: Path, monkeypatch: object) -> None:
        monkeypatch.delenv("XDG_DATA_HOME", raising=False)
        result = get_persist_dir("test-app")
        assert result == Path.home() / ".local" / "share" / "xbin" / "test-app"

    def test_different_app_names(self) -> None:
        d1 = get_persist_dir("app-a")
        d2 = get_persist_dir("app-b")
        assert d1 != d2
        assert "app-a" in str(d1)
        assert "app-b" in str(d2)


class TestEnsurePersistDir:
    """Tests for ensure_persist_dir()."""

    def test_creates_directory(self, tmp_path: Path) -> None:
        os.environ["XDG_DATA_HOME"] = str(tmp_path)
        try:
            result = ensure_persist_dir("new-app")
            assert result.is_dir()
            assert result.exists()
        finally:
            del os.environ["XDG_DATA_HOME"]

    def test_idempotent(self, tmp_path: Path) -> None:
        os.environ["XDG_DATA_HOME"] = str(tmp_path)
        try:
            d1 = ensure_persist_dir("my-app")
            d2 = ensure_persist_dir("my-app")
            assert d1 == d2
            assert d1.is_dir()
        finally:
            del os.environ["XDG_DATA_HOME"]


class TestGetPersistEnv:
    """Tests for get_persist_env()."""

    def test_sets_xbin_persist_dir(self, tmp_path: Path) -> None:
        os.environ["XDG_DATA_HOME"] = str(tmp_path)
        try:
            env = get_persist_env("my-app")
            assert "XBIN_PERSIST_DIR" in env
            assert "my-app" in env["XBIN_PERSIST_DIR"]
        finally:
            del os.environ["XDG_DATA_HOME"]

    def test_directory_exists_after_get_persist_env(self, tmp_path: Path) -> None:
        os.environ["XDG_DATA_HOME"] = str(tmp_path)
        try:
            env = get_persist_env("new-app")
            assert Path(env["XBIN_PERSIST_DIR"]).is_dir()
        finally:
            del os.environ["XDG_DATA_HOME"]
