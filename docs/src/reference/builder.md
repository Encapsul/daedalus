# The Builder

The builder analyzes an application and produces the `.xbin`. It's written in
**pure Python** (stdlib only for the core — optional `cryptography` for
`--encrypt`, optional `tomli` for Python 3.10).

- **Code**: `cli/xbin/build.py`, `cli/xbin/analyzer/`, `cli/xbin/cross.py`
- **Why Python**: the builder is business logic (walking directories,
  parsing ELF headers, manipulating paths, assembling bytes). Python is fast
  to write and modify, and the future AI Analyzer naturally integrates with
  it.

## The three steps

### 1. Analysis — `analyzer/`

- **`runtime.py`** detects the runtime and resolves the entrypoint:
  - `app.py` / `main.py` / `server.py` → **python**;
  - `package.json` → **node**;
  - a single ELF executable → **native binary**.
  - Returns a `RuntimePlan`: interpreter to embed, entrypoint (relative to
    rootfs), `cwd`, `env`, extra directories (e.g. Python stdlib).
- **`elf.py`** analyzes ELF64 binaries directly (without calling `ldd`):
  reads `PT_DYNAMIC` headers, extracts `DT_NEEDED`, `DT_RUNPATH`, `PT_INTERP`,
  and resolves transitive dependencies in standard system search paths
  (`/lib`, `/usr/lib`, `/lib64`, etc.). Includes the dynamic loader
  `ld-linux`.
- **`dockerfile.py`** parses Dockerfile `RUN` instructions to extract
  declared dependencies: apt/apk system packages, pip packages, npm global
  packages, and external binary fetches (wget/curl → tar/unzip → chmod +x
  chains with URL and version). Handles multi-line commands (`\`
  continuations) and `&&`/`;` chains. No Dockerfile → returns `[]`.
- **`python_ast.py`** scans Python source via AST walking to detect
  external binary calls (`subprocess.run`, `os.system`, etc.) that the
  Dockerfile might miss. Extracts literal binary names; dynamic/unresolvable
  names are flagged as `confidence="uncertain"`.
- **`fetch.py`** takes detected dependencies and fetches them into an
  isolated staging directory (`~/.cache/xbin/stage/{hash}/`) without
  touching the real system. Each kind has a dedicated fetcher:
  pip (`pip download --no-deps`), npm (`--prefix`), apt (`apt-get download` +
  `dpkg-deb -x`), apk (`apk fetch --simulate`), external (urllib + extract).
  Records SHA-256 per file in `manifest.json` for auditability. Warns on
  failure but never hard-fails the build.
- **`ldd.py`** is a thin facade that calls `elf.shared_libs()`. Either
  module can be swapped without changing the rest of the builder.
- **Deduplication**: if a library is found via a symlink (e.g.
  `/lib64/ld-linux-x86-64.so.2`) and also via its real path
  (`/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2`), the duplicate is removed to
  keep the rootfs clean.

### 2. Rootfs construction — `_build_runtime_layer()`

Assembles a mini-filesystem containing **exactly** what's needed:

```
rootfs/
  app/                          ← application code
  usr/bin/python3.12            ← interpreter
  usr/lib/python3.12/           ← stdlib
  usr/lib/x86_64-linux-gnu/     ← .so files (libc, etc.)
  lib64/ld-linux-x86-64.so.2    ← dynamic loader
  etc/{passwd,group,hosts,resolv.conf,nsswitch.conf}
```

Key point: we **preserve the absolute tree** of copied files
(`/usr/lib/...` → `rootfs/usr/lib/...`). This lets the embedded Python
interpreter find its stdlib via landmark detection relative to its own path.

For **cross-compilation** (`--target aarch64`), a vendored Python from
`python-build-standalone` is downloaded and `.so` resolution is skipped (no
host libs to resolve for a different arch).

### 3. Layer splitting + compression — `build()`

The builder constructs **two layers** (v2 format):

- **runtime layer** (`_build_runtime_layer`): interpreter + stdlib + `.so` +
  `/etc`. Independent of app code.
- **app layer** (`_build_app_layer`): app code + site-packages. Small and
  volatile.

Each layer is compressed with either `zstd -19` (default) or `mksquashfs`
(`--squashfs` flag, v5 format, better compression ratio).

**Build cache** (`~/.cache/xbin/build/{hash}.zst`): the runtime layer is
looked up by its tar hash. If an identical blob already exists, it's
**reused without recompression** — this is what makes rebuilds (and builds
of apps sharing the same runtime) near-instant.

Final assembly, then `chmod +x`:

```
[ ELF stub ][ runtime layer ][ app layer ][ JSON metadata ][ sig? ][ footer ]
^0          ^payload_offset                              ^meta_offset      ^EOF-92
```

See [`.xbin` Format](./format.md#layers-v2) for the layer table details.

## Typical output

First build (cold build cache):

```
$ xbin build ./examples/bottle-web
[xbin] building 'bottle-web'
  runtime: python
  entrypoint: /usr/bin/python3.12 /app/app.py
  runtime layer: 5 shared libraries
    /lib/x86_64-linux-gnu/libc.so.6
    /lib/x86_64-linux-gnu/libexpat.so.1
    /lib/x86_64-linux-gnu/libm.so.6
    /lib/x86_64-linux-gnu/libz.so.1
    /lib64/ld-linux-x86-64.so.2
  runtime layer: embedded /usr/lib/python3.12
  app layer: site-packages from .../bottle-web/site-packages
  runtime layer: 54.0MB -> 11.9MB (zstd, cached)
  app layer: 0.2MB -> 0.0MB (zstd)
[xbin] wrote ./bottle-web.xbin (7.1MB) in 25.1s
```

Rebuild after code change (runtime layer reused):

```
  runtime layer: reused from build cache (no recompression) ✓
  app layer: 0.2MB -> 0.0MB (zstd)
[xbin] wrote ./bottle-web.xbin (7.1MB) in 1.2s
```

The resolved libraries (5) include the dynamic linker (`ld-linux`) and are
deduplicated: if `/lib64/ld-linux-x86-64.so.2` is a symlink to
`/lib/.../ld-linux-x86-64.so.2`, only one path is kept (the symlink, so
`_copy_into_rootfs` recreates the chain in the rootfs).

## Dependency evolution

The pure-Python ELF analyzer (`elf.py`) replaces the system `ldd` call. It
works on any machine with Python ≥ 3.10, without depending on a specific
`ldd`. It handles ELF64, symlinks, `DT_RUNPATH`, `LD_LIBRARY_PATH`, and
transitive resolution.

**Cross-distro portability**: isolation **level 2** (user namespaces +
`pivot_root`) ensures `/lib64/ld-linux` resolves *inside* the rootfs, not on
the host. See [Isolation](./isolation.md).

## pip install at build time

If the app has a `requirements.txt` with content (non-empty), the builder
automatically creates a temporary venv, pip-installs dependencies, and
embeds them as an additional site-packages entry in `PYTHONPATH`:

```
[xbin] pip install: ./my_app/requirements.txt → /app/site-packages
```
