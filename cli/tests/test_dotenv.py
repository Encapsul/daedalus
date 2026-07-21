"""Tests for dotenv module (.env file parsing and baking)."""

from __future__ import annotations

from pathlib import Path

from xbin.dotenv import detect_secret_keys, load_dotenv, parse_dotenv


class TestParseDotenv:
    """Tests for parse_dotenv()."""

    def test_basic_key_value(self, tmp_path: Path) -> None:
        env_file = tmp_path / ".env"
        env_file.write_text("FOO=bar\nBAZ=qux\n")
        result = parse_dotenv(env_file)
        assert result == {"FOO": "bar", "BAZ": "qux"}

    def test_quoted_values(self, tmp_path: Path) -> None:
        env_file = tmp_path / ".env"
        env_file.write_text('DB_HOST="localhost"\nDB_PASS=\'secret\'\n')
        result = parse_dotenv(env_file)
        assert result["DB_HOST"] == "localhost"
        assert result["DB_PASS"] == "secret"

    def test_export_prefix(self, tmp_path: Path) -> None:
        env_file = tmp_path / ".env"
        env_file.write_text("export API_KEY=abc123\n")
        result = parse_dotenv(env_file)
        assert result == {"API_KEY": "abc123"}

    def test_comments_ignored(self, tmp_path: Path) -> None:
        env_file = tmp_path / ".env"
        env_file.write_text("# this is a comment\nFOO=bar\n# another comment\n")
        result = parse_dotenv(env_file)
        assert result == {"FOO": "bar"}

    def test_empty_lines_ignored(self, tmp_path: Path) -> None:
        env_file = tmp_path / ".env"
        env_file.write_text("\n\nFOO=bar\n\n\n")
        result = parse_dotenv(env_file)
        assert result == {"FOO": "bar"}

    def test_no_variable_expansion(self, tmp_path: Path) -> None:
        env_file = tmp_path / ".env"
        env_file.write_text("FOO=$BAR\nBAZ=${QUX}\n")
        result = parse_dotenv(env_file)
        assert result["FOO"] == "$BAR"
        assert result["BAZ"] == "${QUX}"

    def test_value_with_equals(self, tmp_path: Path) -> None:
        env_file = tmp_path / ".env"
        env_file.write_text("FOO=bar=baz\n")
        result = parse_dotenv(env_file)
        assert result["FOO"] == "bar=baz"

    def test_empty_value(self, tmp_path: Path) -> None:
        env_file = tmp_path / ".env"
        env_file.write_text("FOO=\n")
        result = parse_dotenv(env_file)
        assert result == {"FOO": ""}

    def test_nonexistent_file(self, tmp_path: Path) -> None:
        result = parse_dotenv(tmp_path / "nonexistent.env")
        assert result == {}

    def test_malformed_lines_skipped(self, tmp_path: Path) -> None:
        env_file = tmp_path / ".env"
        env_file.write_text("VALID=yes\n=bad\nBAD_KEY\nFOO=ok\n")
        result = parse_dotenv(env_file)
        assert result == {"VALID": "yes", "FOO": "ok"}


class TestDetectSecretKeys:
    """Tests for detect_secret_keys()."""

    def test_detects_common_patterns(self) -> None:
        env = {
            "DB_PASSWORD": "secret",
            "API_KEY": "abc",
            "TOKEN": "xyz",
            "PRIVATE_KEY": "key",
            "USERNAME": "admin",
        }
        secrets = detect_secret_keys(env)
        assert "DB_PASSWORD" in secrets
        assert "API_KEY" in secrets
        assert "TOKEN" in secrets
        assert "PRIVATE_KEY" in secrets
        assert "USERNAME" not in secrets

    def test_empty_dict(self) -> None:
        assert detect_secret_keys({}) == []


class TestLoadDotenv:
    """Tests for load_dotenv() integration."""

    def test_loads_env_file(self, tmp_path: Path) -> None:
        env_file = tmp_path / ".env"
        env_file.write_text("FOO=bar\nBAZ=qux\n")
        result = load_dotenv(tmp_path, ".env", verbose=False)
        assert result == {"FOO": "bar", "BAZ": "qux"}

    def test_none_env_file_returns_empty(self, tmp_path: Path) -> None:
        assert load_dotenv(tmp_path, None, verbose=False) == {}

    def test_nonexistent_file_returns_empty(self, tmp_path: Path) -> None:
        assert load_dotenv(tmp_path, "nonexistent.env", verbose=False) == {}

    def test_relative_path_resolved(self, tmp_path: Path) -> None:
        env_file = tmp_path / ".env"
        env_file.write_text("KEY=value\n")
        result = load_dotenv(tmp_path, ".env", verbose=False)
        assert result == {"KEY": "value"}

    def test_absolute_path(self, tmp_path: Path) -> None:
        env_file = tmp_path / "custom.env"
        env_file.write_text("KEY=value\n")
        result = load_dotenv(tmp_path, str(env_file), verbose=False)
        assert result == {"KEY": "value"}
