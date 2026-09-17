# Changelog

All notable changes to daedalus are documented here.

## [Unreleased]

### Added

- **Zig runtime** — detects `build.zig`/`build.zig.zon`, auto-downloads the
  stable Zig toolchain from ziglang.org, runs `zig build -Doptimize=ReleaseFast
  -Dtarget=<triple>` and stages the native binary. Cross-compilation supported
  via daedalus `--target` (translated to Zig triples, e.g.
  `x86_64-unknown-linux-gnu` → `x86_64-linux-gnu`).

- **Dart runtime** — detects `pubspec.yaml` (non-Flutter), auto-downloads the
  Dart SDK from dart-archive, runs `dart pub get` + `dart compile exe` (AOT
  compilation). Host-only: cross-compilation is refused with a clear error since
  the AOT compiler only targets the build host.

- **Flutter runtime** — detects `pubspec.yaml` with `sdk: flutter`, runs
  `flutter build <platform> --release` and bundles the desktop release into
  `rootfs/app/bundle/`. Requires the Flutter SDK on PATH (SDK too large to
  auto-download).

- Hub catalog entries for Zig, Dart, and Flutter.

## [1.0.0] — 2026-08-03 — SISR

First major release integrating SISR (Self-Incremental Sovereign
Reconstruction) end to end.

### Added

- **SISR delta self-updates** — binaries embed a content-addressed chunk
  manifest; `./app --daedalus-update <url>` fetches only the changed chunks from
  the update channel and rebuilds the binary in place.
- **`daedalus upgrade-binary`** — promote a legacy (v1, SISR-less) `.daedalus` to the
  v2 format by injecting the delta manifest and `SISR` header, preserving
  every payload byte and the integrity hash.
- **Post-update health gate + automatic rollback** — a newly updated binary
  runs under supervision; a failed health check restores the previous version
  from a snapshot, and quarantined targets are refused.
- **Migration guide** — see [`migration/v1-to-v2.md`](./migration/v1-to-v2.md).

### Compatibility

- v1 (SISR-less) binaries run on the v2 runtime **unchanged and without
  warnings**: standard extraction only, zero startup penalty.
- Deployment scripts based on v1 remain 100 % valid.
- The `.daedalus` footer format is frozen; new capabilities ship as flags and
  backwards-readable format-version bumps.

### Testing

- End-to-end suite against a mock HTTP update channel (`cargo test -p
  daedalus-stub --test e2e_sisr_main`), network fault injection on the
  reconstruction engine, and property-based parser fuzzing via proptest.
- Cross-version tests prove legacy binaries load on the v2 runtime and that
  upgraded binaries gain auto-update.

## [0.3.0] — 2026-07-XX — Pre-SISR

Last release before the SISR integration. Classic `[stub][payload][metadata]
[footer]` binaries only; no delta updates.
