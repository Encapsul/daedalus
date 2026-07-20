"""Unit tests for package manager detection."""

from xbin.pkgmgr import (
    detect_node_pkgmgr,
    detect_pkgmgr,
    detect_python_pkgmgr,
)


class TestPythonPkgMgr:
    def test_uv_lock(self, tmp_path):
        (tmp_path / "uv.lock").write_text("# uv lock file")
        pm = detect_python_pkgmgr(tmp_path)
        assert pm is not None
        assert pm.name == "uv"

    def test_poetry_lock(self, tmp_path):
        (tmp_path / "poetry.lock").write_text("# poetry lock file")
        pm = detect_python_pkgmgr(tmp_path)
        assert pm is not None
        assert pm.name == "poetry"

    def test_pipenv_lock(self, tmp_path):
        (tmp_path / "Pipfile.lock").write_text("{}")
        pm = detect_python_pkgmgr(tmp_path)
        assert pm is not None
        assert pm.name == "pipenv"

    def test_requirements_txt_fallback(self, tmp_path):
        (tmp_path / "requirements.txt").write_text("flask>=2.0\n")
        pm = detect_python_pkgmgr(tmp_path)
        assert pm is not None
        assert pm.name == "pip"

    def test_empty_requirements_txt(self, tmp_path):
        (tmp_path / "requirements.txt").write_text("")
        pm = detect_python_pkgmgr(tmp_path)
        assert pm is None

    def test_uv_beats_poetry(self, tmp_path):
        (tmp_path / "uv.lock").write_text("# uv")
        (tmp_path / "poetry.lock").write_text("# poetry")
        pm = detect_python_pkgmgr(tmp_path)
        assert pm.name == "uv"

    def test_no_lock_no_requirements(self, tmp_path):
        pm = detect_python_pkgmgr(tmp_path)
        assert pm is None


class TestNodePkgMgr:
    def test_pnpm_lock(self, tmp_path):
        (tmp_path / "package.json").write_text('{"name": "test"}')
        (tmp_path / "pnpm-lock.yaml").write_text("lockfileVersion: 1")
        pm = detect_node_pkgmgr(tmp_path)
        assert pm is not None
        assert pm.name == "pnpm"

    def test_yarn_lock(self, tmp_path):
        (tmp_path / "package.json").write_text('{"name": "test"}')
        (tmp_path / "yarn.lock").write_text("# yarn lock")
        pm = detect_node_pkgmgr(tmp_path)
        assert pm is not None
        assert pm.name == "yarn"

    def test_bun_lock(self, tmp_path):
        (tmp_path / "package.json").write_text('{"name": "test"}')
        (tmp_path / "bun.lockb").write_bytes(b"\x00" * 8)
        pm = detect_node_pkgmgr(tmp_path)
        assert pm is not None
        assert pm.name == "bun"

    def test_npm_lock(self, tmp_path):
        (tmp_path / "package.json").write_text('{"name": "test"}')
        (tmp_path / "package-lock.json").write_text("{}")
        pm = detect_node_pkgmgr(tmp_path)
        assert pm is not None
        assert pm.name == "npm"

    def test_pnpm_beats_npm(self, tmp_path):
        (tmp_path / "package.json").write_text('{"name": "test"}')
        (tmp_path / "pnpm-lock.yaml").write_text("lockfileVersion: 1")
        (tmp_path / "package-lock.json").write_text("{}")
        pm = detect_node_pkgmgr(tmp_path)
        assert pm.name == "pnpm"

    def test_no_package_json(self, tmp_path):
        pm = detect_node_pkgmgr(tmp_path)
        assert pm is None

    def test_package_json_no_lock(self, tmp_path):
        (tmp_path / "package.json").write_text('{"name": "test"}')
        pm = detect_node_pkgmgr(tmp_path)
        assert pm is not None
        assert pm.name == "npm"


class TestDetectPkgMgr:
    def test_python_runtime(self, tmp_path):
        (tmp_path / "uv.lock").write_text("# uv")
        pm = detect_pkgmgr(tmp_path, "python")
        assert pm is not None
        assert pm.name == "uv"

    def test_node_runtime(self, tmp_path):
        (tmp_path / "package.json").write_text('{"name": "test"}')
        (tmp_path / "yarn.lock").write_text("# yarn")
        pm = detect_pkgmgr(tmp_path, "node")
        assert pm is not None
        assert pm.name == "yarn"

    def test_unsupported_runtime(self, tmp_path):
        pm = detect_pkgmgr(tmp_path, "java")
        assert pm is None
