# Flutter Applications

Package Flutter applications into self-extracting binaries. daedalus builds the Flutter release bundle and packages it so the app ships as a single file.

## Detection

daedalus detects Flutter applications by finding `sdk: flutter` in the `dev_dependencies` of `pubspec.yaml`.

## How It Works

1. Detects `pubspec.yaml` with a `sdk: flutter` dependency
2. Requires the Flutter SDK on PATH (unlike Zig and Dart, Flutter is too large to auto-download — about 1 GB)
3. Runs `flutter pub get`, then `flutter build <platform> --release`
4. Stages the release bundle into `rootfs/app/bundle/`
5. The `.de` runs the bundle launcher — the Flutter engine is shipped inside the bundle, so no SDK is needed on the target

Bytecode finalization (`--obfuscate --split-debug-info`) is intentionally not applied: the same source tree may be shipped with tree-shaking/minification options from the daedalus CLI.

## Requirements

- The Flutter SDK installed and `flutter` on PATH (https://docs.flutter.dev/get-started/install)
- A `pubspec.yaml` with `sdk: flutter` in `dev_dependencies`
- The platform toolchain for the target: Xcode for macOS/iOS, Visual Studio for Windows, Linux desktop tools (clang, GTK) for Linux

## Example

```bash
mkdir my-flutter-app && cd my-flutter-app
flutter create .

# Add a tiny UI, then package the desktop release:
daedalus build . -o my-flutter-app.de

# Run it
./my-flutter-app.de
```

## Cross-Compilation

`--target` selects the desktop platform for `flutter build`. On the build host:

- `linux` → `flutter build linux --release` (bundle from `build/linux/<arch>/release/bundle/`)
- `windows` → `flutter build windows --release` (runner from `build/windows/x64/runner/Release/`)
- `darwin` → `flutter build macos --release` (app from `build/macos/Build/Products/Release/`)

The daedalus stub is embedded per `--target`; packaging a Flutter app for another OS still requires building the daedalus stub on that OS.

## Notes

- The desktop bundle is self-contained (engine + assets) — no Flutter SDK needed on the target host
- web and mobile targets (`flutter build web`, Android APK/AAB) are not packaged as self-extracting binaries
- Flutter cannot be auto-downloaded by daedalus because of its size and platform requirements — install it once