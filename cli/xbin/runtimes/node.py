"""Node.js runtime detection and embedding.

Supports generic Node.js apps plus framework-specific detection for
Next.js, Nuxt, and Astro.
"""

from __future__ import annotations

import json
import shutil
from pathlib import Path

from . import Runtime, RuntimePlan


class NodeRuntime(Runtime):
    name = "node"

    def detect(self, app_dir: Path) -> RuntimePlan | None:
        if not (app_dir / "package.json").is_file():
            return None
        return _detect_node(app_dir)


def _detect_node(app_dir: Path) -> RuntimePlan:
    node = shutil.which("node")
    if not node:
        raise ValueError(
            "node app detected (package.json) but no node on PATH to embed"
        )
    interp = Path(node).resolve()
    framework = _detect_framework(app_dir)
    entrypoint = _build_entrypoint(app_dir, framework)
    env: dict[str, str] = {}
    site_packages: list[tuple[Path, str]] = []
    nm = app_dir / "node_modules"
    if nm.is_dir():
        site_packages.append((nm, "/app/node_modules"))
        env["NODE_PATH"] = "${ROOTFS}/app/node_modules"
    return RuntimePlan(
        runtime="node",
        interpreter_host=interp,
        entrypoint=entrypoint,
        cwd="/app",
        env=env,
        site_packages=site_packages,
    )


def _rootfs_rel(host_path: Path) -> str:
    return str(host_path).lstrip("/")


def _detect_framework(app_dir: Path) -> str | None:
    """Detect specific Node.js framework from config files and package.json."""
    # Config-file based detection (existing)
    if (
        (app_dir / "next.config.js").is_file()
        or (app_dir / "next.config.mjs").is_file()
        or (app_dir / "next.config.ts").is_file()
    ):
        return "nextjs"
    if (
        (app_dir / "nuxt.config.ts").is_file()
        or (app_dir / "nuxt.config.js").is_file()
        or (app_dir / "nuxt.config.mjs").is_file()
    ):
        return "nuxt"
    if (app_dir / "astro.config.mjs").is_file() or (
        app_dir / "astro.config.ts"
    ).is_file():
        return "astro"
    if (app_dir / "remix.config.js").is_file() or (
        app_dir / "remix.config.mjs"
    ).is_file():
        return "remix"
    if (app_dir / "svelte.config.js").is_file() or (
        app_dir / "svelte.config.ts"
    ).is_file():
        return "sveltekit"

    # Dependency-based detection from package.json
    try:
        pkg = json.loads((app_dir / "package.json").read_text())
        deps = {**pkg.get("dependencies", {}), **pkg.get("devDependencies", {})}
    except (json.JSONDecodeError, OSError):
        return None

    if "next" in deps:
        return "nextjs"
    if "nuxt" in deps:
        return "nuxt"
    if "astro" in deps:
        return "astro"
    if "@remix-run/node" in deps or "@remix-run/react" in deps:
        return "remix"
    if "@sveltejs/kit" in deps:
        return "sveltekit"
    if "express" in deps:
        return "express"
    if "fastify" in deps:
        return "fastify"
    if "hono" in deps:
        return "hono"

    return None


def _build_entrypoint(app_dir: Path, framework: str | None) -> list[str]:
    """Build the entrypoint argv based on detected framework."""
    interp = _node_interp(app_dir)

    if framework == "nextjs":
        return [interp, "/app/node_modules/.bin/next", "start"]

    if framework == "nuxt":
        return [interp, "/app/node_modules/.bin/nuxt", "start"]

    if framework == "astro":
        # Astro SSR: after `astro build`, the server entry is dist/server/entry.mjs
        ssr_entry = app_dir / "dist" / "server" / "entry.mjs"
        if ssr_entry.is_file():
            return [interp, "/app/dist/server/entry.mjs"]
        return [interp, "/app/node_modules/.bin/astro", "start"]

    if framework == "remix":
        # Remix: try build first, fall back to dev
        server_entry = app_dir / "build" / "server" / "index.js"
        if server_entry.is_file():
            return [interp, "/app/build/server/index.js"]
        return [interp, "/app/node_modules/.bin/remix-serve", "build"]

    if framework == "sveltekit":
        # SvelteKit: try build, fall back to dev
        server_entry = app_dir / "build" / "server" / "index.js"
        if server_entry.is_file():
            return [interp, "/app/build/server/index.js"]
        return [interp, "/app/node_modules/.bin/svelte-kit", "dev"]

    if framework == "express":
        # Express: look for app.js or server.js in package.json scripts
        entry = _node_entry(app_dir)
        return [interp, f"/app/{entry}"]

    if framework == "fastify":
        entry = _node_entry(app_dir)
        return [interp, f"/app/{entry}"]

    if framework == "hono":
        # Hono: look for src/index.ts or entry point
        for cand in ("src/index.ts", "src/index.js", "index.ts", "index.js"):
            if (app_dir / cand).is_file():
                return [interp, f"/app/{cand}"]
        return [interp, "/app/index.js"]

    # Generic Node.js: try scripts.start, then common files
    entry = _node_entry(app_dir)
    return [interp, f"/app/{entry}"]


def _node_interp(app_dir: Path) -> str:
    node = shutil.which("node") or "node"
    return f"/{_rootfs_rel(Path(node).resolve())}"


def _node_entry(app_dir: Path) -> str:
    """Find the entry point for a generic Node.js app."""
    try:
        pkg = json.loads((app_dir / "package.json").read_text())

        # Framework-specific: read scripts.start
        scripts = pkg.get("scripts", {})
        start_cmd = scripts.get("start", "")
        if start_cmd and isinstance(start_cmd, str):
            for part in start_cmd.split():
                if part.endswith((".js", ".mjs", ".ts")) and (app_dir / part).is_file():
                    return part

        # Check main field
        main = pkg.get("main")
        if main and (app_dir / main).is_file():
            return main
    except (ValueError, OSError):
        pass

    for cand in ("index.js", "server.js", "app.js"):
        if (app_dir / cand).is_file():
            return cand
    return "index.js"
