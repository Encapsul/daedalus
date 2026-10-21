# Building a Python App

## Expected structure

`daedalus` detects a Python app by the presence of an entry point at the root:
`app.py`, `main.py`, `server.py` or `__main__.py`.

```
my_app/
  app.py            ← entry point, auto-detected
  ...               ← other modules, templates, assets
```

## Build

```bash
daedalus build ./my_app -o my_app.de
```

The builder:

1. detects the `python` runtime and entrypoint (`/app/app.py`);
2. embeds the build machine's `python3` interpreter;
3. embeds the **stdlib** (`/usr/lib/pythonX.Y`);
4. resolves `.so` dependencies via the ELF analyzer (libc, etc.);
5. compresses and assembles the `.de`.

## Environment variables

The builder injects `PYTHONUNBUFFERED=1` (real-time logs) and
`PYTHONDONTWRITEBYTECODE=1` by default. Your app reads its own variables normally:

```python
import os
PORT = int(os.environ.get("PORT", "8080"))
```

```bash
PORT=9000 ./my_app.de
```

## Third-party dependencies (site-packages)

`daedalus` automatically embeds third-party dependencies. It looks, in order:

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
  resolved by the launcher at runtime - see [Launcher](../reference/launcher.md));
- runs the ELF analyzer on `.so` files from C extensions (numpy, pillow...) to
  embed their system dependencies.

The example `examples/bottle-web` demonstrates this: it serves HTTP with
`bottle`, a web framework **not** in the stdlib.

```bash
daedalus build ./examples/bottle-web -o bottle-web.de
./bottle-web.de   # → Hello from bottle, packaged by daedalus
```

## Requirements.txt → pip install at build time

If `requirements.txt` declares at least one installable line (blank lines and
`#` comments do **not** count), and there is no vendored `site-packages/` /
`.venv` yet, the builder runs `pip install --target` into the **staged**
rootfs (`/app/site-packages`) — never into the host environment. Cross builds
also pass `--no-binary :all:` so wheels for the build host are not embedded.

```bash
daedalus build ./my_app -o my_app.de
# [daedalus] pip install: ./my_app/requirements.txt → /app/site-packages
```

Comment-only or whitespace-only `requirements.txt` files are treated as empty
(no pip). Prefer vendoring into `site-packages/` for air-gap builds; see
`examples/complex-python`.

## uv / Poetry / Pipenv → staged site-packages

When `uv.lock`, `poetry.lock`, or `Pipfile.lock` is present and there is no
vendored `site-packages/` / `.venv` yet, the builder **exports** a requirements
file (`uv export`, `poetry export`, or `pipenv requirements`) and runs
`pip install --target` into the staged rootfs — same destination as Pip, never
a silent host-only install. Cross builds still pass `--no-binary :all:`.

If the exporter is missing from `PATH`, or export fails (e.g. Poetry without
`poetry-plugin-export`), the build **errors** with a clear message. Vendor
`site-packages/` or commit a `.venv/` for air-gap builds without those tools.

## Current limitations

- **Cross-distro portability**: see [Isolation](../reference/isolation.md)
  - fully guaranteed at level 2 (user namespaces + pivot_root).
