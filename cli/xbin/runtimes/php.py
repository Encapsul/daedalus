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
        if (app_dir / "composer.json").is_file():
            return _detect_php(app_dir)
        if self._is_wordpress_plugin(app_dir):
            return _detect_php_generic(app_dir)
        return None

    def _is_wordpress_plugin(self, app_dir: Path) -> bool:
        for php_file in app_dir.glob("*.php"):
            try:
                text = php_file.read_text(errors="ignore")
                if "Plugin Name:" in text and "Description:" in text:
                    return True
            except Exception:
                continue
        return False

    def supports_cross(self) -> bool:
        return False


def _detect_php(app_dir: Path) -> RuntimePlan:
    php = shutil.which("php")
    if not php:
        php = _find_php_common()
    if not php:
        try:
            from .downloader import find_php

            php = str(find_php(verbose=False))
        except Exception:
            pass
    if not php:
        raise ValueError("PHP app detected (composer.json) but no php on PATH to embed")
    interp = Path(php).resolve()

    entrypoint = _php_entrypoint(app_dir, interp)

    env: dict[str, str] = {}
    site_packages: list[tuple[Path, str]] = []
    vendor = app_dir / "vendor"
    if vendor.is_dir():
        site_packages.append((vendor, "/app/vendor"))
        env["COMPOSER_AUTOLOAD"] = "${ROOTFS}/app/vendor/autoload.php"

    return RuntimePlan(
        runtime="php",
        interpreter_host=interp,
        entrypoint=entrypoint,
        cwd="/app",
        env=env,
        site_packages=site_packages,
    )


def _detect_php_generic(app_dir: Path) -> RuntimePlan:
    php = shutil.which("php")
    if not php:
        php = _find_php_common()
    if not php:
        try:
            from .downloader import find_php

            php = str(find_php(verbose=False))
        except Exception:
            pass
    if not php:
        raise ValueError("PHP app detected but no php on PATH to embed")
    interp = Path(php).resolve()

    php_cmd = f"/{_rootfs_rel(interp)}"

    if (app_dir / "index.php").is_file():
        entrypoint = [php_cmd, "-S", "0.0.0.0:8000", "-t", "/app"]
    else:
        entrypoint = [php_cmd, "-S", "0.0.0.0:8000", "-t", "/app"]

    return RuntimePlan(
        runtime="php",
        interpreter_host=interp,
        entrypoint=entrypoint,
        cwd="/app",
        env={},
        site_packages=[],
    )


def _rootfs_rel(host_path: Path) -> str:
    return str(host_path).lstrip("/")


def _php_interp(app_dir: Path) -> str:
    php = shutil.which("php") or "php"
    return f"/{_rootfs_rel(Path(php).resolve())}"


def _find_php_common() -> str | None:
    """Check common installation paths for PHP."""
    candidates = [
        Path("/usr/bin/php"),
        Path("/usr/local/bin/php"),
        Path.home() / ".phpbrew" / "php" / "current" / "bin" / "php",
        Path("/opt/php/bin/php"),
    ]
    for c in candidates:
        if c.is_file():
            return str(c)
    return None


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
