# Dependency Detection

This is where ~70% of the complexity lives — and `xbin`'s differentiator.

## What the ELF analyzer sees

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

Advantage: no `ldd` required on the host machine — works anywhere Python runs,
ideal for cross-compilation and self-hosting.

## What static analysis does NOT see

Many dependencies are only discovered at **runtime**:

```python
subprocess.run(["ffmpeg", "-i", src, dst])          # external binary
ctypes.cdll.LoadLibrary("libcuda.so.1")              # dynamic dlopen
importlib.import_module(plugin_name)                 # dynamically loaded plugin
os.system("convert in.png out.jpg")                  # ImageMagick
```

No **static** tool can reliably find these: you need to *understand* the code,
not just read its symbol table.

## Two modes

- **`auto`** (default): best-effort detection via ELF analysis + runtime heuristics.
- **`manifest`**: the user explicitly declares external binaries, `dlopen` libs,
  required env vars, and data files in an `xbin.toml`. This is the safety net
  when auto detection is not enough.

```toml
# xbin.toml (target)
[deps]
binaries = ["ffmpeg", "convert"]
libraries = ["libcuda.so.1"]   # optional, GPU
[env]
required = ["DATABASE_URL", "SECRET_KEY", "PORT"]
```

## The role of AI (differentiator)

AI solves **one** problem, but a real one: detecting these hidden dependencies.
It analyzes the source code and finds `subprocess`, `dlopen`, dynamic plugins,
required environment variables, then **generates an `xbin.toml`** that the user
reviews before building.

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
