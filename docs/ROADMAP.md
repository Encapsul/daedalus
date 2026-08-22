# daedalus — ROADMAP (docs)

## Vision

daedalus est un **format d'artefact exécutable universel**, capable de transporter :

- une application web/server/CLI ✅
- un service/microservice — objectif
- un plugin/extension — objectif

> Le format est conçu comme une unité autonome = stub + runtime + payload + metadata + signature.
> Rien n'est hérité du système hôte.

---

## Use case clé : Universal Application Packaging

Une application web est un binaire daedalus qui contient :

```
[stub][runtime (python3)][code (app/)][config (daedalus.toml)][deps (site-packages/)]
```

**Ce que le format permet aujourd'hui :**
- Packager runtime + code + config dans un seul binaire portable
- Signature Ed25519 pour vérifier l'origine
- Chiffrement AES-256-GCM pour protéger le payload
- SISR pour update delta sans tout re-télécharger

**Ce qui manque :**
- **Hot-swap de couches** : remplacer la couche `RuntimeLayer` (nouveau runtime) sans extraire le code.
- **Lazy loading** : ne charger les gros assets que lorsqu'ils sont appelés.
- **Multi-service** : un seul binaire contenant 2-3 services avec un entrypoint par service.

> Sans intégration des couches dans le stub, l'hot-swap et le lazy loading ne sont pas possibles.

---

## État actuel (août 2026)

### Couche cœur (`daedalus-core`)
- ✅ CAS — `cas.rs` : `ObjectStore` trait, `MemoryStore`, `DiskObjectStore`
- ✅ Format binaire — `format.rs` : v2 (plain), v3 (signed), v4 (encrypted), v5 (squashfs)
- ✅ Metadata — `metadata.rs` : `Metadata` avec runtime, entrypoint, layers
- ✅ Signature Ed25519 — `crypto.rs`
- ✅ Chiffrement AES-256-GCM — `encrypt.rs`
- ✅ Détection runtime — `detect.rs` : 11 runtimes, `EntrypointRegistry`

### Abstraction des couches (`layer.rs`)
- ✅ Types concrets : `RuntimeLayer`, `ConfigLayer`
- ✅ `Capability` enum : `ReadFile`, `WriteFile`, `Network`, `Exec`, `Syscall`, `Env`
- ✅ `LayerKind` : `Runtime`, `Config`, `Custom`
- ✅ `LayerManifest` : tracking des couches dans le cache du stub

### SISR / Delta updates (`sisr/`)
- ✅ `SisrEngine` : réutilisation de chunks, vérification SHA-256, swap atomique
- ✅ `HealthStore` : quarantaine, suivi des échecs
- ✅ `AtomicWriter` : remplacement atomique de fichiers
- ✅ Tests réseau avec injection de fautes

### Assemblage
- ✅ `assemble_daedalus` : builder unifié avec `AtomicWriter`, aware SISR
- ✅ Manifest distant : `.daedalus.manifest`

### CLI (`daedalus-cli`)
- ✅ `build`, `inspect`, `scan`, `sign`, `verify`, `keygen`, `trust`
- ✅ `doctor`, `env`, `clean`, `completion`, `man`, `upgrade`
- ✅ `registry push/pull/list` — content-addressable layer sharing
- ✅ `swap` — hot-swap layers without rebuild
- ✅ `publish` — publish layers to local/remote CAS after build

### Stub launcher (`daedalus-stub`)
- ✅ Lecture footer/metadata depuis `/proc/self/exe`
- ✅ Cache SHA-256, vérification d'intégrité
- ✅ Extraction zstd+tar ou squashfs
- ✅ `execvp` entrypoint
- ✅ SISR delta update
- ✅ Layer manifest tracking (cache-aware warm start)
- ✅ Capability-based sandboxing (seccomp + Landlock)

---

## Ce qui reste à faire

### Phase 1 : Universal Binary (bloquant)

**Objectif** : Un `.daedalus` marche partout (x64/ARM64, Linux/macOS/Windows).

1. `--universal` flag → build matrix via `cargo zigbuild` pour chaque OS/arch
2. Assemble slices dans wrapper polyglot (MZ+ELF+Mach-O overlap header)
3. Runtime : detect `uname -s`/`uname -m` → extract right slice → `execve`

**Status** : Cross-compilation validée (seccomp.rs fix). Wrapper polyglot à implémenter.

### Phase 2 : Hot-swap (3-4 jours)

**Objectif** : Remplacer une couche sans rebuild complet du binaire.

1. `daedalus swap <binary> <layer-name> <new-layer-file>`
2. SISR-aware : si activé, génère un delta update

### Phase 3 : Lazy loading (4-5 jours)

**Objectif** : Ne charger que les couches nécessaires, à la demande.

1. Mapping mémoire des couches — mmap le payload extrait
2. Montage FUSE optionnel — monter le rootfs sans extraction complète

### Phase 4 : Registry CAS (3-4 jours)

**Objectif** : Publier/charger des couches via un registre distant.

1. `Registry` dans `daedalus-core` au-dessus de `ObjectStore`
2. CLI `daedalus registry push/pull/list`
3. Intégrer au build : `daedalus build --publish`

### Phase 5 : Security audit + enforcement (2-3 jours)

**Objectif** : Les `Capability` doivent être vérifiées, pas juste déclarées.

1. Enforcer les capabilities dans le stub (seccomp + Landlock)
2. Tests de sandboxing

### Phase 6 : Templates + multi-service (2-3 jours)

**Objectif** : Support pour services et plugins.

1. Templates de metadata : `application`, `service`, `plugin`
2. Multi-service : `daedalus build ./services --entrypoint api=api.py --entrypoint worker=worker.py`

---

## Priorité

1. **Phase 1** (universal binary) — **bloque cross-platform**
2. **Phase 5** (security) — **nécessaire avant production**
3. **Phase 2** (hot-swap) — **différenciant**, remplacer une couche sans rebuild
4. **Phase 3** (lazy loading) — **performance**, ne pas charger gros assets au démarrage
5. **Phase 4** (registry) — **adoption**, partager des couches entre artefacts
6. **Phase 6** (templates) — **storytelling**, prouve que le format est universel

---

## Contraintes techniques

- **vfat** : le repo vit sur vfat (pas de bit exec). Les artefacts build vont dans `/tmp/daedalus-stub-target`.
- **musl** : le stub compile avec `--target x86_64-unknown-linux-musl` pour un ELF statique.
- **CI** : clippy est lancé par crate, pas workspace-wide.
- **Edition** : Rust 2021, stable uniquement (pas de nightly).
- **Sécurité** : zero `unsafe` dans `daedalus-core` et `daedalus-cli`. Toute la logique unsafe est dans `daedalus-stub`.
