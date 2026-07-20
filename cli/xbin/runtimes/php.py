"""PHP runtime detection and embedding."""

from __future__ import annotations

import shutil
from pathlib import Path

from . import Runtime, RuntimePlan


class PHPRuntime(Runtime):
    name = "php"

    def detect(self, app_dir: Path) -> RuntimePlan | None:
        if not (app_dir / "composer.json").is_file():
            return None
        return _detect_php(app_dir)

    def supports_cross(self) -> bool:
        return False


def _detect_php(app_dir: Path) -> RuntimePlan:
    php = shutil.which("php")
    if not php:
        raise ValueError("PHP app detected (composer.json) but no php on PATH to embed")
    interp = Path(php).resolve()

    entry = _php_entry(app_dir)

    env: dict[str, str] = {}
    vendor = app_dir / "vendor"
    if vendor.is_dir():
        env["COMPOSER_AUTOLOAD"] = "${ROOTFS}/app/vendor/autoload.php"

    return RuntimePlan(
        runtime="php",
        interpreter_host=interp,
        entrypoint=[f"/{_rootfs_rel(interp)}", f"/app/{entry}"],
        cwd="/app",
        env=env,
    )


def _rootfs_rel(host_path: Path) -> str:
    return str(host_path).lstrip("/")


def _php_entry(app_dir: Path) -> str:
    # Framework-specific detection
    if (app_dir / "artisan").is_file():
        # Laravel
        return "artisan"
    if (app_dir / "symfony.lock").is_file() or (
        app_dir / "config" / "bundles.php"
    ).is_file():
        # Symfony
        return "bin/console"
    if (app_dir / "wp-config.php").is_file():
        # WordPress
        return "wp-cli.phar"
    if (app_dir / "public" / "index.php").is_file():
        # Generic framework with public/ directory
        return "public/index.php"
    if (app_dir / "index.php").is_file():
        return "index.php"
    return "index.php"
