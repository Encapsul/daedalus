# Roadmap

## Vision

Un **format d'artefact exécutible universel** : `[polyglot-stub][payload][metadata][footer]`.
Un seul fichier `.daedalus` qui contient le runtime + le code + les dépendances + la config,
signé Ed25519, avec delta updates Sisir et sandbox seccomp+Landlock. Fonctionne sur Linux x86_64/ARM64,
macOS ARM64, Windows x64 — sans installer quoi que ce soit sur la cible.

**Use case clé :** Packager n'importe quelle application web, serveur, ou CLI en un seul fichier exécutable portable.

## Positioning YC

> "Daedalus packages any application — Python, Node, Go, PHP, Ruby, .NET, Java, Deno, Perl,
> Hugo — into a single self-extracting binary that runs anywhere without Docker or a runtime
> install. While PyInstaller only does Python and Bun only does JS/TS, Daedalus supports 14
> runtimes in one tool, with 95% smaller updates via SISR."

### Qui a ce problème?

Un **développeur (solo ou petite équipe)** qui construit une **application web/service/CLI**
et veut la distribuer à des utilisateurs qui n'ont rien installé — ni Docker, ni Python,
ni Node, ni quoi que ce soit.

### Pourquoi maintenant?

- Docker nécessite un daemon + runtime installé — lourd pour une simple app
- PyInstaller/pkg/Bun sont mono-langue — un projet full-stack nécessite plusieurs outils
- Les mises à jour téléchargent l'artefact complet — SISR télécharge seulement les chunks modifiés

### Pourquoi toi?

- Architecture conçue pour ce cas : multi-runtime detection (Python/Node/Go/PHP/etc.),
  SISR FastCDC pour delta updates, sandbox seccomp+Landlock
- Code existant: build + inspect + run + swap + registry + Sisir delta + signing + encrypt
- Format ouvert : un `.daedalus` peut être décompressé avec `tar`/`unsquashfs`
- Expérience construite : 423 tests passent, code Rust 2021 stable, ANSSI-compliant

## Phase 1 — MVP universal packaging ✅

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
- [x] Minimal seccomp filter (18 syscalls bloqués, arch-aware x86_64/aarch64)
- [x] Payload encryption AES-256-GCM (v4)
- [x] SquashFS extraction (v5 format, `--squashfs`)
- [x] 14 runtimes supportés : Python, Node.js, Deno, Java, Ruby, .NET, Go, PHP, Perl, Hugo, Wasm, Electron, Rust, Binary
- [x] Framework auto-detection (FastAPI, Flask, Django, Next.js, Express, etc.)

## Phase 2 — Delta updates ✅

- [x] **SISR engine** : FastCDC content-defined chunking + Merkle delta manifest
- [x] Content-addressed build cache (runtime layer reused, rebuild ~25s → ~1s)
- [x] Chunk reuse entre apps partageant le meme runtime
- [x] Bandwidth reporting : `"12 MB delta vs 328 MB full — 96.4%"`
- [x] `--enable-sisr` flag + incremental rebuild via `daedalus build --update`

## Phase 3 — Universal Binary ✅

- [x] `daedalus build --universal` : build matrix multi-arch via `cargo zigbuild`
- [x] Polyglot launcher shell-script avec offsets hardcoded
- [x] `UniversalFooter` + manifest JSON par slice
- [x] Détection runtime `uname -m`/`uname -s` → extraction de la bonne slice
- [x] Slices Linux x86_64 + aarch64, macOS ARM64, Windows x64

## Phase 4 — Hot-swap ✅

- [x] `daedalus swap <binary> <layer-name> <new-file>` : remplacer une couche sans rebuild
- [x] Limitation documentée : zstd-tar v2 seulement, invalide la signature Ed25519
- [x] Tests : remplacement de fichier, rejet de SISR binaire

## Phase 5 — Lazy loading ✅

- [x] `--lazy-load` flag on `daedalus build`
- [x] Priority extraction in stub: entrypoint + runtime interpreter extracted first
- [x] Background thread continues extracting remaining files after `.ready`
- [x] `.lazy_done` marker when background extraction completes
- [x] Works with zstd+tar payloads (squashfs falls back to full extraction)

## Phase 6 — Templates + multi-service (PARTIAL)

- [x] Champ `services` dans metadata, détection mode serveur dans `daedalus selftest`
- [ ] Build multi-service : `daedalus build ./services --entrypoint api=api.py --entrypoint worker=worker.py`
- [ ] Templates de metadata : `application`, `service`, `plugin`

## Priorité

1. **Phase 5** (lazy loading) — **performance**, ne pas charger gros assets au démarrage
2. **Phase 6** (templates + multi-service) — **storytelling**, prouve que le format est universel
3. **Registry CAS** — **adoption**, partager des couches entre artefacts (`daedalus registry push/pull/list` déjà implémenté)

## Ce qui n'est PAS prioritaire

- ❌ OCI/WASM (`--to oci|appimage|wasm`) — abandonné (dilution, cf Section 7)
- ❌ Desktop packaging (flatpak/snap) — hors scope
- ❌ 50 repos GitHub trending — integration testing, pas un feature produit
- ❌ Metrics Prometheus — nice-to-have ops, pas un feature produit
- ❌ Edge/IoT OTA channels — SISR le couvre déjà (delta updates + signatures)

## Section 7 — À NE PAS FAIRE (conflits résolus)

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
