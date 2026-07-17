# Dependency Detection

This is where ~70% of the complexity lives — and `xbin`'s differentiator.

## Three-layer detection pipeline

The builder combines three complementary detection methods, each catching
what the others miss:

```
Dockerfile ──→ dockerfile.py ──┐
                                ├──→ merge_deps() ──→ fetch.py ──→ staging
Python source ──→ python_ast.py ┘
```

### Layer 1: Dockerfile analysis (`dockerfile.py`)

Parses `RUN` instructions to extract declared dependencies:

- **apt/apk packages**: `apt-get install -y ffmpeg libssl-dev`
- **pip packages**: `pip install flask==2.0 requests>=2.25`
- **npm global packages**: `npm install -g typescript`
- **External binary fetches**: detects the full chain
  `wget/curl → tar/unzip → chmod +x` with URL and version extraction

Handles real-world Dockerfiles: multi-line commands with `\` continuations,
`&&`/`;` chains, `#` comments. No Dockerfile → returns `[]` (graceful
degradation).

### Layer 2: Python source AST scanning (`python_ast.py`)

Walks Python AST to find `subprocess.run`, `os.system`, `os.popen` and
related calls that the Dockerfile might miss:

```python
# This call is detected by AST scanning:
subprocess.run(["ffmpeg", "-i", src, dst])
os.system("convert in.png out.jpg")
```

Literal string and list arguments are extracted with high confidence.
Dynamic or unresolvable names (variables, f-strings) are flagged as
`confidence="uncertain"` — reported but never fetched.

### Layer 3: Dependency fetching (`fetch.py`)

Detected dependencies are fetched into an isolated staging directory
(`~/.cache/xbin/stage/{hash}/`) without touching the real system:

| Kind | Method | Never does |
|---|---|---|
| pip | `pip download --no-deps --dest` | global install |
| npm | `npm install --prefix` | global node_modules |
| apt | `apt-get download` + `dpkg-deb -x` | `apt-get install` |
| apk | `apk fetch --simulate` + extract | `apk add` |
| external | `urllib` download + extract | system modification |

Each fetch records SHA-256 in `manifest.json` for auditability. Failures
warn but never hard-fail the build.

## What static analysis does NOT see

Even with all three layers, some dependencies remain invisible to static
analysis:

```python
ctypes.cdll.LoadLibrary("libcuda.so.1")              # dynamic dlopen
importlib.import_module(plugin_name)                 # dynamically loaded plugin
```

No **static** tool can reliably find these: you need to *understand* the code,
not just read its symbol table.

## Two modes

- **`auto`** (default): best-effort detection via Dockerfile + AST + ELF
  analysis.
- **`manifest`**: the user explicitly declares external binaries, `dlopen`
  libs, required env vars, and data files in an `xbin.toml`. This is the
  safety net when auto detection is not enough.

```toml
# xbin.toml (target)
[deps]
binaries = ["ffmpeg", "convert"]
libraries = ["libcuda.so.1"]   # optional, GPU
[env]
required = ["DATABASE_URL", "SECRET_KEY", "PORT"]
```

## The ELF analyzer

The builder uses a **pure-Python ELF analyzer** (`analyzer/elf.py`) that reads
binary headers directly:

1. Iterates `Program Headers` to find `PT_DYNAMIC` and `PT_INTERP`
2. Extracts `DT_NEEDED` entries (required libraries) and `DT_RUNPATH` (search
   paths)
3. For each library, searches standard paths (`/lib`, `/usr/lib`, `/lib64`,
   etc.), `LD_LIBRARY_PATH`, and `DT_RUNPATH`
4. Recursively resolves dependencies of each found `.so`
5. Deduplicates entries pointing to the same real file via different symlinks

```
your binary ─ELF→ libc.so.6, libssl.so.3, ..., ld-linux-x86-64.so.2
```

No `ldd` required on the host machine — works anywhere Python runs,
ideal for cross-compilation and self-hosting.

## The role of AI (differentiator)

AI solves **one** problem, but a real one: detecting hidden dependencies that
even the AST scanner can't resolve (variables, config-driven binaries,
`dlopen`). It analyzes the source code and generates an `xbin.toml` that the
user reviews before building.

```
xbin build ./my_app --ai-analyze
[xbin AI] Runtime: Python 3.11 / FastAPI
  External binaries: ffmpeg (subprocess), convert (os.system)
  Dynamic libs:      libcuda.so.1 (ctypes, optional)
  Env required:      DATABASE_URL, SECRET_KEY, PORT
Generated xbin.manifest. Review before building.
```

> This is the only place where AI provides what no static tool can do.
> Status: **Phase 3** — the interface (`--ai-analyze` → `xbin.toml`) is
> designed so AI is a *manifest generator*, never an opaque mandatory step.
