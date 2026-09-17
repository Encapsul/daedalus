# Zig Applications

Package Zig applications into self-extracting binaries. daedalus builds your Zig code into a native binary and packages it.

## Detection

daedalus detects Zig applications by looking for `build.zig` or `build.zig.zon` in the project root. Zig modules without a `build.zig.zon` (pre-0.15 layout with `addExecutable` in `build.zig`) are also supported.

## How It Works

1. Detects `build.zig`/`build.zig.zon` in the project directory
2. Locates the `zig` toolchain: system `zig` on PATH, otherwise the latest stable release is auto-downloaded into `~/.cache/daedalus/build-tools/zig-host/`
3. Runs `zig build -Doptimize=ReleaseFast -Dtarget=<triple>` with the target translated from `--target`
4. Stages the produced executable into `rootfs/app/` and strips it
5. No runtime interpreter needed — signature `../hello-zig` style entrypoints exec the binary directly

## Requirements

- `build.zig` (with a `createModule`-based exe step or classic `addExecutable`) in the project root
- Optionally `build.zig.zon` in the project root
- Zig toolchain: installed on PATH or auto-downloaded (you get `zig` in the cache, no manual step)

### Pinning a Zig version

Set `DAEDALUS_ZIG_VERSION=<x.y.z>` to pin a specific Zig release. Without it, the newest stable is auto-selected from the ziglang.org manifest.

## Example

```bash
mkdir my-zig-app && cd my-zig-app

cat > build.zig << 'EOF'
const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});
    const exe = b.addExecutable(.{
        .name = "app",
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/main.zig"),
            .target = target,
            .optimize = optimize,
        }),
    });
    b.installArtifact(exe);
}
EOF

mkdir src
cat > src/main.zig << 'EOF'
const std = @import("std");

pub fn main() !void {
    std.debug.print("Hello from Zig!\n", .{});
}
EOF

# Build the .de
daedalus build . -o my-zig-app.de

# Run it
./my-zig-app.de
```

You can also use the ready-made example in this repository:

```bash
daedalus build examples/hello-zig -o hello-zig.de
```

## Cross-Compilation

Zig is a self-contained cross-compiler (no C cross-toolchains needed for many targets). `--target` is translated to Zig triples:

| `--target` | Zig triple |
| --- | --- |
| `x86_64-unknown-linux-gnu` | `x86_64-linux-gnu` |
| `aarch64-unknown-linux-musl` | `aarch64-linux-musl` |
| `aarch64-apple-darwin` | `aarch64-macos` |
| `x86_64-pc-windows-msvc` | `x86_64-windows` |

```bash
daedalus build . --target x86_64-unknown-linux-gnu -o my-zig-app-linux.de
```

## Notes

- The `.de` runs `zig build` entrypoint as a native binary — no `zig` needed on the target host
- Zig resolves its `lib/` directory relative to the executable, so the auto-downloaded toolchain is kept in place in the cache (never moved)
- `strip_compiled_sources` applies to the Zig output