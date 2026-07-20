"""Python runtime detection and embedding.

Supports generic Python apps plus Django (manage.py + wsgi.py/asgi.py).
"""

from __future__ import annotations

import shutil
import sys
from pathlib import Path

from . import Runtime, RuntimePlan


class PythonRuntime(Runtime):
    name = "python"

    def detect(self, app_dir: Path) -> RuntimePlan | None:
        # Django detection: manage.py + wsgi.py/asgi.py
        if (app_dir / "manage.py").is_file():
            django_plan = _detect_django(app_dir)
            if django_plan is not None:
                return django_plan

        entry = _first_existing(
            app_dir, ["app.py", "main.py", "__main__.py", "server.py"]
        )
        if not entry:
            return None
        return _detect_python(app_dir, entry)

    def supports_cross(self) -> bool:
        return True


def _first_existing(app_dir: Path, names: list[str]) -> str | None:
    for n in names:
        if (app_dir / n).is_file():
            return n
    return None


def _detect_django(app_dir: Path) -> RuntimePlan | None:
    """Detect Django project and configure gunicorn/uvicorn entrypoint."""
    wsgi_module = _find_django_wsgi(app_dir)
    asgi_module = _find_django_asgi(app_dir)

    if not wsgi_module and not asgi_module:
        return None

    py = shutil.which("python3") or shutil.which("python") or sys.executable
    interp = Path(py).resolve()
    stdlib = _python_stdlib(interp)

    env = {"PYTHONUNBUFFERED": "1", "PYTHONDONTWRITEBYTECODE": "1"}
    site_packages: list[tuple[Path, str]] = []
    sp_host = _find_site_packages(app_dir)
    if sp_host:
        site_packages.append((sp_host, "/app/site-packages"))
        env["PYTHONPATH"] = "${ROOTFS}/app/site-packages"

    # Prefer gunicorn > uvicorn > python manage.py runserver
    gunicorn = shutil.which("gunicorn")
    uvicorn = shutil.which("uvicorn")

    if gunicorn and wsgi_module:
        ginterp = Path(gunicorn).resolve()
        entrypoint = [
            f"/{_rootfs_rel(ginterp)}",
            f"{wsgi_module}:application",
            "--bind",
            "0.0.0.0:8000",
            "--workers",
            "4",
        ]
    elif uvicorn and asgi_module:
        uinterp = Path(uvicorn).resolve()
        entrypoint = [
            f"/{_rootfs_rel(uinterp)}",
            f"{asgi_module}:application",
            "--host",
            "0.0.0.0",
            "--port",
            "8000",
        ]
    else:
        # Fallback: manage.py runserver
        entrypoint = [
            f"/{_rootfs_rel(interp)}",
            "/app/manage.py",
            "runserver",
            "0.0.0.0:8000",
        ]

    return RuntimePlan(
        runtime="python",
        interpreter_host=interp,
        entrypoint=entrypoint,
        cwd="/app",
        env=env,
        extra_dirs_host=[stdlib] if stdlib else [],
        site_packages=site_packages,
    )


def _find_django_wsgi(app_dir: Path) -> str | None:
    """Find Django WSGI module path (e.g. 'myproject.wsgi')."""
    for candidate in app_dir.rglob("wsgi.py"):
        parts = list(candidate.relative_to(app_dir).parts)
        if len(parts) >= 2:
            module = ".".join(parts[:-1]) + ".wsgi"
            return module
    return None


def _find_django_asgi(app_dir: Path) -> str | None:
    """Find Django ASGI module path (e.g. 'myproject.asgi')."""
    for candidate in app_dir.rglob("asgi.py"):
        parts = list(candidate.relative_to(app_dir).parts)
        if len(parts) >= 2:
            module = ".".join(parts[:-1]) + ".asgi"
            return module
    return None


def _detect_python(app_dir: Path, py_entry: str) -> RuntimePlan:
    py = shutil.which("python3") or shutil.which("python") or sys.executable
    interp = Path(py).resolve()
    stdlib = _python_stdlib(interp)

    env = {"PYTHONUNBUFFERED": "1", "PYTHONDONTWRITEBYTECODE": "1"}
    site_packages: list[tuple[Path, str]] = []
    sp_host = _find_site_packages(app_dir)
    if sp_host:
        site_packages.append((sp_host, "/app/site-packages"))
        env["PYTHONPATH"] = "${ROOTFS}/app/site-packages"

    return RuntimePlan(
        runtime="python",
        interpreter_host=interp,
        entrypoint=[f"/{_rootfs_rel(interp)}", f"/app/{py_entry}"],
        cwd="/app",
        env=env,
        extra_dirs_host=[stdlib] if stdlib else [],
        site_packages=site_packages,
    )


def _rootfs_rel(host_path: Path) -> str:
    return str(host_path).lstrip("/")


def _find_site_packages(app_dir: Path) -> Path | None:
    for venv_name in (".venv", "venv"):
        venv = app_dir / venv_name
        lib = venv / "lib"
        if lib.is_dir():
            for pyd in sorted(lib.glob("python*")):
                sp = pyd / "site-packages"
                if sp.is_dir():
                    return sp
    vendored = app_dir / "site-packages"
    if vendored.is_dir():
        return vendored
    return None


def _python_stdlib(interp: Path) -> Path | None:
    candidate = (
        Path(sys.base_prefix)
        / "lib"
        / f"python{sys.version_info.major}.{sys.version_info.minor}"
    )
    if candidate.is_dir():
        return candidate
    return None
