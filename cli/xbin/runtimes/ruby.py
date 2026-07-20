"""Ruby runtime detection and embedding.

Detection: Gemfile or *.rb with Bundler present.
Entry point: ruby <script>.rb or ruby -e <code>.
"""

from __future__ import annotations

import shutil
from pathlib import Path

from . import Runtime, RuntimePlan


class RubyRuntime(Runtime):
    name = "ruby"

    def detect(self, app_dir: Path) -> RuntimePlan | None:
        if (app_dir / "Gemfile").is_file():
            return _detect_bundler(app_dir)
        return _detect_single_file(app_dir)


def _find_ruby() -> Path:
    ruby = shutil.which("ruby")
    if not ruby:
        raise ValueError(
            "Ruby project detected but no ruby on PATH. "
            "Install Ruby (e.g. ruby-full)."
        )
    return Path(ruby).resolve()


def _ruby_entry(app_dir: Path) -> str:
    """Find the main Ruby script."""
    for cand in ("main.rb", "app.rb", "server.rb", "config.ru", "Rakefile"):
        if (app_dir / cand).is_file():
            return cand
    # Check config/ directory (Rails convention)
    config_ru = app_dir / "config" / "ru"
    if config_ru.is_file():
        return "config/ru"
    return "main.rb"


def _detect_bundler(app_dir: Path) -> RuntimePlan:
    ruby_bin = _find_ruby()
    entry = _ruby_entry(app_dir)
    env: dict[str, str] = {}
    extra_dirs: list[Path] = []
    gems_dir = app_dir / "vendor" / "bundle"
    if gems_dir.is_dir():
        extra_dirs.append(gems_dir)
        env["GEM_PATH"] = "${ROOTFS}/app/vendor/bundle"
    elif (app_dir / ".bundle").is_dir():
        # Bundler local install
        gem_dir = _bundler_gem_dir(app_dir)
        if gem_dir and gem_dir.is_dir():
            extra_dirs.append(gem_dir)
            env["GEM_PATH"] = str(gem_dir)
    return RuntimePlan(
        runtime="ruby",
        interpreter_host=ruby_bin,
        entrypoint=[f"/{_rootfs_rel(ruby_bin)}", f"/app/{entry}"],
        cwd="/app",
        env=env,
        extra_dirs_host=extra_dirs,
    )


def _detect_single_file(app_dir: Path) -> RuntimePlan | None:
    rb_files = list(app_dir.glob("*.rb"))
    if not rb_files:
        return None
    # Only detect if there's exactly one .rb file
    if len(rb_files) != 1:
        return None
    # Skip test files
    if rb_files[0].name.startswith("test_"):
        return None
    ruby_bin = _find_ruby()
    return RuntimePlan(
        runtime="ruby",
        interpreter_host=ruby_bin,
        entrypoint=[f"/{_rootfs_rel(ruby_bin)}", f"/app/{rb_files[0].name}"],
        cwd="/app",
    )


def _rootfs_rel(host_path: Path) -> str:
    return str(host_path).lstrip("/")


def _bundler_gem_dir(app_dir: Path) -> Path | None:
    """Read BUNDLE_PATH from .bundle/config."""
    config = app_dir / ".bundle" / "config"
    if not config.is_file():
        return None
    try:
        for line in config.read_text().splitlines():
            if "BUNDLE_PATH:" in line:
                path = line.split(":", 1)[1].strip()
                return (app_dir / path).resolve()
    except OSError:
        pass
    return None
