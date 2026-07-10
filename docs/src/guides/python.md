# Building a Python App

## Expected structure

`xbin` detects a Python app by the presence of an entry point at the root:
`app.py`, `main.py`, `server.py` or `__main__.py`.

```
my_app/
  app.py            ← entry point, auto-detected
  ...               ← other modules, templates, assets
```

## Build

```bash
xbin build ./my_app -o my_app.xbin
```

The builder:

1. detects the `python` runtime and entrypoint (`/app/app.py`);
2. embeds the build machine's `python3` interpreter;
3. embeds the **stdlib** (`/usr/lib/pythonX.Y`);
4. resolves `.so` dependencies via the pure-Python ELF analyzer (libc, etc.);
5. compresses and assembles the `.xbin`.

## Environment variables

The builder injects `PYTHONUNBUFFERED=1` (real-time logs) and
`PYTHONDONTWRITEBYTECODE=1` by default. Your app reads its own variables normally:

```python
import os
PORT = int(os.environ.get("PORT", "8080"))
```

```bash
PORT=9000 ./my_app.xbin
```

## Third-party dependencies (site-packages)

`xbin` automatically embeds third-party dependencies. It looks, in order:

1. a virtualenv `.venv/` or `venv/` at the app root → its
   `lib/pythonX.Y/site-packages`;
2. a vendored `site-packages/` directory at the app root.

```
my_app/
  app.py
  .venv/                ← auto-detected, site-packages embedded
    lib/python3.12/site-packages/
      bottle.py
```

The builder:

- copies site-packages to `/app/site-packages` in the rootfs;
- declares `PYTHONPATH=${ROOTFS}/app/site-packages` (the `${ROOTFS}` token is
  resolved by the launcher at runtime — see [Launcher](../reference/launcher.md));
- runs the ELF analyzer on `.so` files from C extensions (numpy, pillow...) to
  embed their system dependencies.

The example `examples/bottle-web` demonstrates this: it serves HTTP with
`bottle`, a web framework **not** in the stdlib.

```bash
xbin build ./examples/bottle-web -o bottle-web.xbin
./bottle-web.xbin   # → Hello from bottle, packaged by xbin
```

## Requirements.txt → pip install at build time

If your app has a `requirements.txt` with non-empty content, the builder
automatically creates a temporary venv, pip-installs the dependencies, and
embeds them:

```bash
xbin build ./my_app -o my_app.xbin
# [xbin] pip install: ./my_app/requirements.txt → /app/site-packages
```

## Current MVP limitations

- **Cross-distro portability**: see [Isolation](../reference/isolation.md)
  — fully guaranteed at level 2 (user namespaces).
