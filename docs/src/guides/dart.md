# Dart Applications

Package Dart CLI applications into self-extracting binaries. daedalus AOT-compiles your Dart program into a native executable and packages it.

## Detection

daedalus detects Dart applications by finding a `pubspec.yaml` without a Flutter `sdk: flutter` dependency. The entrypoint is resolved in this order:

1. `bin/<script in pubspec>` (`dart run` default, e.g. `bin/main.dart`)
2. `bin/main.dart`
3. `lib/main.dart`
4. The first `bin/*.dart` file

## How It Works

1. Detects `pubspec.yaml` (no Flutter sdk dependency) and picks the entrypoint
2. Locates the toolchain: system `dart` on PATH, otherwise the latest stable Dart SDK is auto-downloaded into `~/.cache/daedalus/build-tools/dart-host/`
3. Runs `dart pub get` to resolve dependencies
4. AOT-compiles the entrypoint with `dart compile exe`
5. Stages the native executable into `rootfs/app/`
6. The `.de` runs the compiled binary directly — no Dart runtime needed on the target

## Requirements

- `pubspec.yaml` with a `name` and a Dart entrypoint (`bin/` or `lib/`)
- Dart toolchain: installed on PATH or auto-downloaded

### Pinning a Dart SDK version

Set `DAEDALUS_DART_VERSION=<x.y.z>` to pin a specific SDK release. Without it, the newest stable is auto-selected from the dart-archive latest manifest.

## Example

```bash
mkdir my-dart-app && cd my-dart-app

cat > pubspec.yaml << 'EOF'
name: my_dart_app
description: A Dart CLI packaged by daedalus.
publish_to: none

environment:
  sdk: ">=3.0.0 <4.0.0"

dependencies: {}
EOF

mkdir bin
cat > bin/main.dart << 'EOF'
import 'dart:io';

void main() {
  stdout.writeln('Hello from Dart!');
}
EOF

# Build the .de
daedalus build . -o my-dart-app.de

# Run it
./my-dart-app.de
```

You can also use the ready-made example in this repository:

```bash
daedalus build examples/hello-dart -o hello-dart.de
```

## Cross-Compilation

Dart AOT (`dart compile exe`) only targets the build host — it cannot produce foreign-platform binaries. Passing `--target` for a different OS or architecture aborts the build with a clear error. Use a per-platform CI job or a native build per target instead.

## Notes

- The compiled `.de` is a fully native binary — no Dart SDK needed on the target host
- Package dependencies are fetched at build time; the graph is resolved by `dart pub get`
- `strip_compiled_sources` applies to the Dart output
- Flutter apps (pubspec with `sdk: flutter`) are handled by the [`Flutter guide`](./flutter.md)