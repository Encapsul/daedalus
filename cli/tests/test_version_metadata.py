"""Tests for version metadata embedding."""

from __future__ import annotations

from pathlib import Path

from xbin.assembly import build_meta_json


class TestVersionMetadata:
    """Tests for version metadata in build_meta_json."""

    def _default_kwargs(self) -> dict:
        return {
            "name": "test-app",
            "runtime": "python",
            "isolation": 0,
            "entrypoint": ["python3", "app.py"],
            "env": {},
            "layers": [],
        }

    def test_version_included(self) -> None:
        import json

        meta = json.loads(build_meta_json(**self._default_kwargs(), version="1.2.3"))
        assert meta["version"] == "1.2.3"

    def test_author_included(self) -> None:
        import json

        meta = json.loads(build_meta_json(**self._default_kwargs(), author="John"))
        assert meta["author"] == "John"

    def test_description_included(self) -> None:
        import json

        meta = json.loads(
            build_meta_json(**self._default_kwargs(), description="A test app")
        )
        assert meta["description"] == "A test app"

    def test_license_included(self) -> None:
        import json

        meta = json.loads(build_meta_json(**self._default_kwargs(), license="MIT"))
        assert meta["license"] == "MIT"

    def test_version_omitted_when_empty(self) -> None:
        import json

        meta = json.loads(build_meta_json(**self._default_kwargs()))
        assert "version" not in meta
        assert "author" not in meta
        assert "description" not in meta
        assert "license" not in meta

    def test_all_metadata_together(self) -> None:
        import json

        meta = json.loads(
            build_meta_json(
                **self._default_kwargs(),
                version="2.0.0",
                author="Alice",
                description="My app",
                license="Apache-2.0",
            )
        )
        assert meta["version"] == "2.0.0"
        assert meta["author"] == "Alice"
        assert meta["description"] == "My app"
        assert meta["license"] == "Apache-2.0"
