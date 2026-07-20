"""C# / .NET runtime detection and embedding.

Detection: *.csproj or *.sln present.
Entry point: dotnet <dll> or dotnet run (for dev projects).
"""

from __future__ import annotations

import shutil
from pathlib import Path
from xml.etree import ElementTree as ET

from . import Runtime, RuntimePlan


class DotnetRuntime(Runtime):
    name = "dotnet"

    def detect(self, app_dir: Path) -> RuntimePlan | None:
        csproj = _find_csproj(app_dir)
        if csproj is None:
            return None
        return _detect_dotnet(app_dir, csproj)


def _find_csproj(app_dir: Path) -> Path | None:
    """Find the main .csproj file."""
    # Direct match
    for csproj in app_dir.glob("*.csproj"):
        return csproj
    # Look in subdirectories
    for csproj in app_dir.rglob("*.csproj"):
        # Skip test projects
        if "test" in csproj.stem.lower():
            continue
        return csproj
    return None


def _find_dotnet() -> Path:
    dotnet = shutil.which("dotnet")
    if not dotnet:
        raise ValueError(
            ".NET project detected but no dotnet on PATH. "
            "Install .NET SDK (e.g. dotnet-sdk-8.0)."
        )
    return Path(dotnet).resolve()


def _parse_output_type(csproj: Path) -> str:
    """Extract OutputType from .csproj (Exe, Library, etc.)."""
    try:
        tree = ET.parse(csproj)
        root = tree.getroot()
        ns = ""
        if root.tag.startswith("{"):
            ns = root.tag.split("}")[0] + "}"
        for prop in root.iter(f"{ns}OutputType"):
            if prop.text:
                return prop.text.strip()
    except (ET.ParseError, OSError):
        pass
    return "Exe"


def _dll_name(csproj: Path) -> str:
    """Derive the DLL name from the .csproj filename."""
    return csproj.stem + ".dll"


def _detect_dotnet(app_dir: Path, csproj: Path) -> RuntimePlan:
    dotnet_bin = _find_dotnet()
    output_type = _parse_output_type(csproj)
    dll = _dll_name(csproj)
    env: dict[str, str] = {}
    extra_dirs: list[Path] = []

    # For published apps, check for publish/ directory
    publish_dir = app_dir / "publish"
    if not publish_dir.is_dir():
        publish_dir = app_dir / "bin" / "Release" / "net8.0" / "publish"
    if not publish_dir.is_dir():
        publish_dir = app_dir / "bin" / "Release" / "net9.0" / "publish"

    if publish_dir.is_dir():
        # Published self-contained or framework-dependent
        published_dll = publish_dir / dll
        if published_dll.is_file():
            entrypoint = [f"/{_rootfs_rel(dotnet_bin)}", f"/app/publish/{dll}"]
        else:
            # Find any DLL in publish dir
            dlls = list(publish_dir.glob("*.dll"))
            if dlls:
                entrypoint = [
                    f"/{_rootfs_rel(dotnet_bin)}",
                    f"/app/publish/{dlls[0].name}",
                ]
            else:
                entrypoint = [f"/{_rootfs_rel(dotnet_bin)}", f"/app/publish/{dll}"]
        extra_dirs.append(publish_dir)
    elif output_type.lower() == "library":
        raise ValueError(
            f".NET class library detected ({csproj.name}) — cannot run a library directly. "
            "Add an executable entry point or use xbin.toml manifest."
        )
    else:
        # Use dotnet run (requires SDK in the image)
        entrypoint = [
            f"/{_rootfs_rel(dotnet_bin)}",
            "run",
            "--project",
            f"/app/{csproj.name}",
        ]

    return RuntimePlan(
        runtime="dotnet",
        interpreter_host=dotnet_bin,
        entrypoint=entrypoint,
        cwd="/app",
        env=env,
        extra_dirs_host=extra_dirs,
    )


def _rootfs_rel(host_path: Path) -> str:
    return str(host_path).lstrip("/")
