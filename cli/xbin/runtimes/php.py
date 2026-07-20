"""PHP runtime detection and embedding.

Supports Laravel, Symfony, WordPress, and generic PHP apps.
"""

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

    entrypoint = _php_entrypoint(app_dir, interp)

    env: dict[str, str] = {}
    vendor = app_dir / "vendor"
    if vendor.is_dir():
        env["COMPOSER_AUTOLOAD"] = "${ROOTFS}/app/vendor/autoload.php"

    return RuntimePlan(
        runtime="php",
        interpreter_host=interp,
        entrypoint=entrypoint,
        cwd="/app",
        env=env,
    )


def _rootfs_rel(host_path: Path) -> str:
    return str(host_path).lstrip("/")


def _php_interp(app_dir: Path) -> str:
    php = shutil.which("php") or "php"
    return f"/{_rootfs_rel(Path(php).resolve())}"


def _php_entrypoint(app_dir: Path, interp: Path) -> list[str]:
    """Build the entrypoint argv based on detected framework."""
    php_cmd = f"/{_rootfs_rel(interp)}"

    # Laravel: php artisan serve
    if (app_dir / "artisan").is_file():
        return [php_cmd, "/app/artisan", "serve", "--host=0.0.0.0", "--port=8000"]

    # Symfony: php bin/console
    if (app_dir / "symfony.lock").is_file() or (
        app_dir / "config" / "bundles.php"
    ).is_file():
        return [php_cmd, "/app/bin/console", "server:run", "0.0.0.0:8000"]

    # WordPress: PHP built-in server
    if (app_dir / "wp-config.php").is_file():
        return [
            php_cmd,
            "-S",
            "0.0.0.0:8080",
            "-t",
            "/app",
        ]

    # Generic framework with public/ directory
    if (app_dir / "public" / "index.php").is_file():
        return [php_cmd, "-S", "0.0.0.0:8000", "-t", "/app/public"]

    if (app_dir / "index.php").is_file():
        return [php_cmd, "-S", "0.0.0.0:8000", "-t", "/app"]

    return [php_cmd, "-S", "0.0.0.0:8000", "-t", "/app"]
