# Roadmap

## Vision

Un **ELF Linux** : `[stub][payload][metadata][footer]`.
Le CLI tourne sur Linux, macOS et Windows. L'artefact tourne sur Linux
(WSL ou VM ailleurs). Le smoke CI est `examples/hello-web`.

**Use case clé :** packager une appli dont le runtime n'est pas installé sur
la machine cible, et pouvoir vérifier la signature avec une clé publique
que l'émetteur a transmise.

## Positioning

> "Daedalus packages an app into one self-extracting Linux ELF. The CI-proven
> case is a Python stdlib HTTP server. Other runtimes are detected. Delta
> updates exist; the only published size ratio is a synthetic blob, not a
> production model."

### Qui a ce problème?

Un développeur qui veut remettre un serveur Linux à quelqu'un qui n'a pas
Docker, ni le runtime, déjà installés.

### Pourquoi maintenant?

- Docker demande un daemon
- PyInstaller et pkg sont mono-langage
- Un hello-node daedalus est plus lent au premier lancement qu'AppImage ou pkg
  (2,1 s et 123 Mio extraits, une passe). Le créneau est l'appli que ces
  outils ne packagent pas, pas le hello-world

### Pourquoi ce dépôt?

- Détection multi-runtime, SISR, signature Ed25519 stricte
- Le format est un ELF plus un tar/zstd ou un SquashFS
- Le smoke CI est un seul exemple. Ne pas annoncer 20 runtimes en production

## Phase 1 - MVP universal packaging ✅

- [x] `.daedalus` format défini et stable (v2-v5 : plain, signed, encrypted, squashfs)
- [x] Stub ELF statique (musl), SHA-256 integrity verification, execvp entrypoint
- [x] Extraction atomique via `rename()` avec `flock()` pour concurrence
- [x] Python runtime detection + rootfs construction (Flask/FastAPI/Django)
- [x] Node.js end-to-end support (stdlib + node_modules)
- [x] Deno support (deno.json detection)
- [x] Ed25519 signatures (v3) + trust model (`daedalus keygen/sign/verify/trust`)
- [x] Level 2 isolation : user namespaces + `pivot_root`
- [x] Smart `.so` deduplication
- [x] `/etc/hosts` dans rootfs
- [x] Dockerfile dependency detection (apt/apk/pip/npm + binary fetch chains)
- [x] PATH injection (binaires bundle trouvés au runtime)
- [x] Filtre seccomp : denylist de 18 syscalls sur x86_64 (17 ailleurs), pas un profil d'application
- [x] Payload encryption AES-256-GCM (v4)
- [x] SquashFS extraction (v5 format, `--squashfs`)
- [x] 20 détecteurs de runtime. Smoke CI : Python stdlib (`examples/hello-web`). Le reste est une détection, un exemple dans `examples/`, ou un test e2e conditionnel (Rust, Java)

## Phase 2 - Delta updates ✅

- [x] **SISR engine** : FastCDC content-defined chunking + Merkle delta manifest
- [x] Content-addressed build cache (runtime layer reused, rebuild ~25s → ~1s)
- [x] Chunk reuse entre apps partageant le meme runtime
- [x] Bandwidth reporting dans le moteur SISR. Le ratio « 12 MB vs 328 MB, 96,4 % » n'apparaît dans aucun test : ne pas le citer comme mesure
- [x] `--enable-sisr` flag + incremental rebuild via `daedalus build --update`

## Phase 3 - Universal Binary ✅

- [x] `daedalus build --universal` : build matrix multi-arch via `cargo zigbuild`
- [x] Polyglot launcher shell-script avec offsets hardcoded
- [x] `UniversalFooter` + manifest JSON par slice
- [x] Détection runtime `uname -m`/`uname -s` → extraction de la bonne slice
- [x] Slices Linux x86_64 + aarch64. Des slices macOS et Windows peuvent être embarquées derrière le lanceur polyglot. Chacune reste un binaire de sa plateforme. Le smoke CI ne couvre pas ces slices

## Phase 4 - Hot-swap ✅

- [x] `daedalus swap <binary> <layer-name> <new-file>` : remplacer une couche sans rebuild
- [x] Limitation documentée : zstd-tar v2 seulement, invalide la signature Ed25519
- [x] Tests : remplacement de fichier, rejet de SISR binaire

## Phase 5 - Lazy loading ✅

- [x] `--lazy-load` flag on `daedalus build`
- [x] Priority extraction in stub: entrypoint + runtime interpreter extracted first
- [x] Background thread continues extracting remaining files after `.ready`
- [x] `.lazy_done` marker when background extraction completes
- [x] Works with zstd+tar payloads (squashfs falls back to full extraction)

## Phase 6 - Templates + multi-service ✅

- [x] Champ `services` dans metadata, détection mode serveur dans `daedalus selftest`
- [x] Build multi-service : `daedalus build ./services --entrypoint service=cmd`
- [x] Templates de metadata : `daedalus build --template application|service|plugin`, recordé dans `meta["template"]` pour le discovery par les outils/hôtes

## Priorité

1. **Cross-platform CI** - prouver « ça tourne partout » : windows-check, cross windows-gnu, native-macos, smoke tests (2ab5d7b + e75c97a corrigent les dernières races ; cargo-audit vert depuis rustls 0.23.45, native-macos vert) - reste windows-check core test + smoke-test-windows, bloqués par l'accès aux logs CI
2. **Adoption** — démo `examples/hello-web` (le guide 60 s). Ne plus citer clinic-agent comme mesure Gemma. Installateurs : `install.sh`, `install.ps1`, Homebrew, `daedalux` sur crates.io et PyPI. Hub : exemples buildables + recettes non exécutées en CI.
3. **Runtimes supplémentaires** ✅ - Dart/Flutter et Zig ajoutés (écosystème dev + single binary). Zig : auto-téléchargement toolchain + AOT natif, e2e validé. Dart : auto-téléchargement SDK + `dart compile exe`, e2e validé. Flutter : bundle desktop via `flutter build`, code présent (Flutter SDK requis, pas d'auto-download du SDK). Guides : [`zig`](./guides/zig.md), [`dart`](./guides/dart.md), [`flutter`](./guides/flutter.md), 3 recettes hub.

## Runtimes supplémentaires - livré

- **Zig** : détection (`build.zig` / `build.zig.zon`), auto-téléchargement toolchain (ziglang.org manifest, tri par version stable), `zig build -Doptimize=ReleaseFast -Dtarget=<triple>`, auto-zig implicit.
- **Dart** : détection (`pubspec.yaml` sans `sdk: flutter`), auto-téléchargement SDK (dart-archive), `dart compile exe` (AOT). Cross-compilation refusée (host-only, message explicite).
- **Flutter** : détection (`sdk: flutter`), `flutter build <platform> --release`, bundle dans `rootfs/app/bundle/`. Flutter SDK requis (pas d'auto-download - trop volumineux).

## Adoption - livré

- **Démo 60 s** : [guide](./guides/demo-60s.md) — `examples/hello-web`. L'ancien chiffre clinic-agent (build 6,2 s, artefact 16,9 MB, Gemma) décrivait un script sans poids de modèle. Il ne faut plus le citer.
- **Installation** : `install.sh` à la racine (Linux/macOS, sans sudo, sha256 obligatoire contre `checksums.txt`). `scripts/install.sh` est l'ancienne copie, elle refuse aussi un checksum absent. Tap Homebrew `Encapsul/homebrew-daedalus`. PyPI et crates.io : `daedalux` (le nom `daedalus` est pris), binaire `daedalus`.
- **Hub** : [hub/catalog.json](../../hub/catalog.json). Buildables dans le repo : hello-web, hello-node, bottle-web, hello-zig, hello-dart, hello-lua, et clinic-agent (scaffold sans poids). Les lignes Express, FastAPI, Spring, Rails, Laravel, Go, Flutter sont des recettes, pas des runs CI.
- **Stub discovery** : `daedalus build` trouve désormais le stub natif (living next to the CLI, ou triple hôte) sans `DAEDALUS_STUB_PATH`.

## Ce qui n'est PAS prioritaire

- ❌ OCI/WASM (`--to oci|appimage|wasm`) - abandonné (dilution, cf Section 7)
- ❌ Desktop packaging (flatpak/snap) - hors scope
- ❌ 50 repos GitHub trending - integration testing, pas un feature produit
- ❌ Metrics Prometheus - nice-to-have ops, pas un feature produit
- ❌ Edge/IoT OTA channels - SISR le couvre déjà (delta updates + signatures)

## Section 7 - À NE PAS FAIRE (conflits résolus)

### OCI multi-format output (Position B deprecation)

Le `--to [oci|appimage|wasm]` (Position B) est **déprécié**.
Raisons : diluition de focus, l'OCI n'est concurrent qu'au-delà du réseau,
et le format `.daedalus` existant répond déjà au besoin de distribution.

## Stack technique (août 2026)

- **Rust 2021** stable, `opt-level="z"`, LTO, strip, `panic=abort`
- **stub** : musl static ELF, ~100KB, `unsafe` limité (FFI + seccomp BPF)
- **core/cli** : zero `unsafe`, ANSSI-Rust compliant
- **CI** : `cargo zigbuild` cross-compile, `cargo clippy -p {crate}`, `cargo test --workspace`
- **Tests** : 423 unit/integration/e2e, QEMU pour cross-arch validation
