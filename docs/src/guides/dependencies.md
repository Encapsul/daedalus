# Dependency Detection

This is where the builder's complexity lives — detecting what the app needs
and bundling exactly the right files.

## Detection pipeline

The Rust builder (`daedalus-core`) combines runtime detection with package
manager detection to resolve dependencies:

```
app directory ──→ detect.rs ──→ runtime + entrypoint
                  pkgmgr.rs ──→ package manager + install strategy
                  assembly  ──→ copy interpreter + libs + app into rootfs
```

### Runtime detection (`detect.rs`)

Identifies the runtime and resolves the entrypoint:

- `app.py` / `main.py` / `server.py` → **Python**
- `package.json` → **Node.js**
- `deno.json` / `deno.jsonc` → **Deno**
- `pom.xml` / `build.gradle` → **Java**
- `Gemfile` → **Ruby**
- `*.csproj` / `*.sln` → **.NET/C#**
- `go.mod` → **Go**
- `composer.json` → **PHP**
- `Makefile.PL` / `cpanfile` → **Perl**
- `hugo.toml` / `hugo.yaml` → **Hugo**
- ELF executable → **Binary**

### Package manager detection (`pkgmgr.rs`)

Detects which package manager an app uses (speed-based priority):

| Runtime | Priority order |
|---------|---------------|
| Python | uv > poetry > pipenv > pip |
| Node.js | pnpm > yarn > bun > npm |

### ELF dependency resolution

For dynamic binaries, the builder reads ELF64 program headers directly
(without calling `ldd`):

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

No `ldd` required on the host machine — works with any Rust toolchain,
ideal for cross-compilation and self-hosting.

## What static analysis does NOT see

Even with runtime detection, some dependencies remain invisible to static
analysis:

```python
ctypes.cdll.LoadLibrary("libcuda.so.1")              # dynamic dlopen
importlib.import_module(plugin_name)                 # dynamically loaded plugin
```

No **static** tool can reliably find these: you need to *understand* the code,
not just read its symbol table.

## Configuration override

Users can explicitly declare external binaries, `dlopen` libs, required env
vars, and data files in an `daedalus.toml`. This is the safety net when auto
detection is not enough.

```toml
# daedalus.toml (target)
[deps]
binaries = ["ffmpeg", "convert"]
libraries = ["libcuda.so.1"]   # optional, GPU
[env]
required = ["DATABASE_URL", "SECRET_KEY", "PORT"]
```
