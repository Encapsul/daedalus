# Packaging a Native Binary

`daedalus` can package a pre-compiled ELF binary into a self-extracting executable.
No runtime embedding is needed — just the binary and its shared libraries.

## Detection

The builder detects a native binary when a single ELF executable is found in the
app directory and no other runtime (Python, Node, Java, etc.) is detected.

```
my-binary/
  my-app          ← ELF executable (auto-detected)
  libfoo.so       ← optional shared library
```

## Build

```bash
daedalus build ./my-binary -o my-binary.de
```

The builder:

1. detects the ELF binary via the `\x7fELF` magic bytes;
2. resolves its shared library dependencies via the ELF analyzer;
3. packages the binary + libraries into the app layer;
4. compresses and assembles the `.de`.

## How it works

No runtime interpreter is embedded. The launcher:

1. extracts the binary and its `.so` dependencies to the cache;
2. sets `LD_LIBRARY_PATH` to include the extracted library directories;
3. `execvp`s the binary directly.

## Shared library resolution

The Rust ELF analyzer reads `DT_NEEDED`, `DT_RPATH`, and `DT_RUNPATH`
entries from the ELF header to find all transitive shared library dependencies.
No host `ldd` is required.

```bash
daedalus inspect my-binary.de
# layers:
#   - app    2.1MB compressed / 8.4MB raw  (binary + .so files)
```

## Cross-compilation

Native binaries cannot be cross-compiled by `daedalus` — the binary must already be
built for the target architecture. Use `daedalus build --target aarch64` only for
interpreted runtimes (Python, etc.) where the interpreter can be downloaded for
the target arch.

## Known limitations

- Only ELF binaries are supported (no PE/Windows, no Mach-O/macOS).
- Statically linked binaries (no `.so` dependencies) work perfectly — the
  smallest possible `.de`.
- Binaries with unusual loader paths (`/lib/ld-linux.so.2`) may need the
  loader embedded in the rootfs.
