# daedalus — ROADMAP

## Vision

Un **format d'artefact exécutible universel** : `[polyglot-stub][payload][metadata][footer]`.
Un seul fichier `.daedalus` qui contient le runtime + le code + les dépendances + la config,
signé Ed25519, avec delta updates Sisir et sandbox seccomp+Landlock. Marche sur Linux x86_64 /
ARM64, macOS ARM64, Windows x64 — sans installer quoi que ce soit sur la cible.

**Use case clé :** Packager n'importe quelle application web, serveur, ou CLI en un seul
fichier exécutable portable.

## Positioning YC

> "Daedalus packages any application — Python, Node, Go, PHP, Ruby, .NET, Java, Deno, Perl,
> Hugo — into a single self-extracting binary that runs anywhere without Docker or a runtime
> install. While PyInstaller only does Python and Bun only does JS/TS, Daedalus supports 11
> runtimes in one tool, with 95% smaller updates via SISR."

### Qui a ce problème?

Un **développeur (solo ou petite équipe)** qui construit une application web/service/CLI
et veut la **distribuer** à des utilisateurs ou collègues qui n'ont rien installé —
ni Docker, ni Python, ni Node, ni quoi que ce soit.

### Pourquoi maintenant?

- Docker nécessite un daemon + runtime installé — lourd pour une simple app
- PyInstaller (Python) / Bun (JS/TS) / pkg (Node) sont mono-langue — un projet full-stack
  nécessite plusieurs outils
- Les mises à jour téléchargent l'artefact complet — SISR télécharge seulement les chunks modifiés

### Pourquoi toi?

- Architecture conçue pour ce cas : multi-runtime detection (Python/Node/Go/PHP/etc.),
  SISR FastCDC pour delta updates, encryption AES-256-GCM, sandbox seccomp+Landlock
- Code existant: build + inspect + run + swap + registry + Sisir delta + signing + encrypt
- Format ouvert : un `.daedalus` peut être décompressé avec `tar`/`unsquashfs`

---

## Modèle économique — Analyse des 6 angles

| # | Modèle | Viabilité | Effort démarrage | Alignement sécurité |
|---|---|---|---|---|
| **2** | **Consulting / hardening service** | **Immédiate** | **Faible** | **Maximum** |
| **6** | **Security testing sandbox** | Moyenne | Faible | Maximum |
| **1** | **Open core / enterprise features** | Moyenne | Moyen | Élevé |
| **3** | **IA agents edge** | Haute (marché tendance) | Moyen | Faible (on viens de DEGACER le code IA) |
| **5** | **Licensing IoT/embedded vendors** | Moyenne | Élevé | Moyen |
| **4** | **Marketplace / registry** | Faible (chicken-and-egg) | Élevé | Élevé |

### Décision : plan cohérent en 3 phases

**Phase 0 (0-3 mois) — Consulting + preuve de marché**
- Lancer le **consulting/hardening service** (model #2) : packager + sécuriser des apps clients
- Proposer le **security testing sandbox** (model #6) : distraire des PoCs/pentest dans un binary signé + sandboxé
- **IoT/Edge focus** : ciblanger les cas d'usage concrets identifiés par le terrain
  (edge appliances de remplacement, diagnostic tools pour hardware deprecated, security tooling
  pour IP cameras/routeurs) — packaging + sandbox + signing = valeur immédiate
- Chaque mission = feedback sur les features enterprise réellement valorisées
- Objectif : 2-3 clients payants → data pour productiser

**Phase 1 (3-9 mois) — Open core**
- Based on consulting feedback, ship enterprise features as paid tier:
  - **Pro** ($7/user/mois) : private registry, SISR, Ed25519 signing
  - **Enterprise** (sur mesure) : airgap, AES-256-GCM encryption, Landlock avancé, attestation, CI/CD plugins, support + SLA
- Consulting = canal lead generation pour le produit

**Phase 2 (9-18 mois) — Scale**
- Universal packaging = angle principal (YC positioning)
- Marketplace/registry **seulement si** la phase 1 prouve la demande
- IoT/embedded licensing **seulement si** le produit est adopté par des vendors

### Ce qu'on ne fait PAS (et pourquoi)

- **IA agents edge (model #3)** — on vient de DEGACER tout le code IA. Le "packaging universel" couvre déjà le besoin (Python + modèles embarqués). Pas de refocus AI.
- **Marketplace registry (model #4)** — chicken-and-egg, nécessite un réseau existant. Trop tôt.
- **OCI/AppImage/WASM export** — deprecated (voir §5). Le format `.daedalus` single-file est le produit.

---

## Priorité du CEO

1. **Universal Binary** — un `.daedalus` marche partout (x64/ARM64, Linux/macOS/Windows)
2. **Cross-runtime layer sharing** — deux apps partageant le même runtime = même SHA256 → cache partagé → cold start < 200ms
3. **Lazy loading** — mmap/FUSE pour charger les gros assets (model files, datasets, etc.) à la demande via SISR chunks

## Ce qui est TERMINÉ (août 2026)

- ✅ Format binaire v2-v5 (plain, signed, encrypted, squashfs) + SHA-256 + Ed25519
- ✅ SISR delta engine (FastCDC content-defined chunking + Merkle delta manifest)
- ✅ Sandbox sécurité (seccomp 18 syscalls + Landlock + capabilités)
- ✅ 11 runtimes supportés (Python, Node, Deno, Java, Ruby, .NET, Go, PHP, Perl, Binary, Hugo)
- ✅ Framework auto-detection (FastAPI, Flask, Django, Next.js, Express, etc.)
- ✅ Cross-compilation validée (cargo zigbuild + fix seccomp.rs pour aarch64)
- ✅ Self-hosting : `daedalus build self/` → CLI packages elle-même
- ✅ Docker dependency detection (apt/apk/pip/npm + binary fetch chains)
- ✅ PATH injection, smart `.so` deduplication, `/etc/hosts` dans rootfs
- ✅ Ed25519 signatures + trust model (`daedalus keygen/sign/verify/trust`)
- ✅ Payload encryption AES-256-GCM (v4 format)
- ✅ Health checks (`daedalus health`)
- ✅ Persistent storage + data files + tree-shaking + minification
- ✅ Content-addressable layer registry (push/pull/list, local + remote)
- ✅ Layer manifest tracking in stub (cache-aware warm start)
- ✅ 423 tests passent + 5 chaos monkey tests (symlink traversal, env var leakage, crypto rejection) ✅, fmt/clippy clean, Rust 2021 stable, zero unsafe en dehors du stub

## Ce qui reste — PRIORITÉ ABSOLUE

### 0. Universal Binary (3-4 jours) — 🔥 Priorité #1

**Problème :** Aujourd'hui `daedalus build` produit un binaire **architecture-specific**.
Le stub x64 ne marche pas sur ARM64.

**Solution :** Polyglot launcher multi-format.

```
foo.daedalus (universal file)
├─ [polyglot-stub]        ← ELF+PE+Mach-O header overlap
├─ [linux-x64-payload]    ← stub x64 + runtime x64 + app x64
├─ [linux-arm64-payload]  ← stub arm64 + runtime arm64 + app arm64
├─ [macos-arm64-payload]  ← Mach-O stub + runtime arm64
├─ [windows-x64-payload]  ← PE stub + runtime x64
├─ metadata (per-arch)
└─ footer + signatures
```

**Implémentation :**
1. ✅ `--universal` flag → build matrix via `cargo zigbuild` pour chaque OS/arch
2. ✅ Assemble slices dans wrapper polyglot (shell-script launcher, page-aligned)
3. ✅ Runtime : detect `uname -s`/`uname -m` → extract right slice via `dd` → `exec`
4. ✅ Test via Docker+QEMU (`qemu-aarch64-static` + `docker --platform linux/arm64`)

**Format :**
```
[shell-script launcher (64 KiB page-aligned)]  ← detects arch, dd-extracts slice
[x86_64 .daedalus slice]                       ← ELF e_machine=EM_X86_64
[aarch64 .daedalus slice]                      ← ELF e_machine=EM_AARCH64
[JSON manifest (4 KiB)]                        ← arch → offset/size/sha256
[universal footer (26 B)]                      ← magic+BEEFCABE, offsets
```

**Cross-arch validation :**
- x86_64 : ✅ shell launcher → dd extract → exec works (Node.js app ran)
- aarch64 : ✅ ELF header e_machine=EM_AARCH64 confirmed at correct offset
- Future: MZ+ELF+Mach-O overlap header for Windows+macOS (APE/Cosmopolitan style)

**Status :** ✅ Implémenté et validé (2 archs Linux musl, tests unitaires + smoke test x86_64).

### 1. Hot-swap (Phase 2) — 3-4 jours

`daedalus swap <binary> <layer> <new-file>` — remplacer une couche sans rebuild.
Exemple : `daedalus swap myapp.daedalus runtime ./new-venv`

### 2. Layer sharing entre apps — 2-3 jours

Deux apps avec le même runtime = même SHA256 → cache partagé → cold start < 200ms.
`daedalus build ./app --publish ~/.daedalus/registry` → layers: [RuntimeLayer, AppLayer]

### 3. Lazy loading (Phase 3) — 4-5 jours

mmap/FUSE pour ne pas charger les gros assets au démarrage. Chargé à la demande via SISR chunks.

### 4. Build cache (local + remote) — 1-2 jours

**État** : flags `--use_cache`, `--clear_cache`, `--remote_cache_url` existent dans `args.rs` mais rien n'est implémenté.

**Implémentation** :
1. Cache local : hash du payload + `config_fingerprint()` → skip assemblage si hit
2. Remote cache : HTTP PUT/GET sur `{url}/{hash}` pour partager les builds entre CI
3. Invalidation : changement de config ou de source → nouveau fingerprint

**Bénéfice** : CI/CD ne rebuild pas l'identique à chaque run. Cross-compilation Python/Go (5-10 min) évitée si rien n'a changé.

### 5. `daedalus-runtime` lib (extraction du stub) — 2-3 jours

**État** : `daedalus-stub/src/extraction.rs`, `exec.rs`, `seccomp.rs`, `landlock.rs`, `namespace.rs` sont des modules bien séparés.

**Implémentation** :
1. Créer `daedalus-runtime/` avec `[lib]`
2. Déplacer les modules du stub vers la lib
3. Exposer l'API publique :
   - `verify(path) -> Result<Footer>` — vérifie l'intégrité sans extraire
   - `extract(path, dest) -> Result<Rootfs>` — extrait le payload
   - `launch(meta, rootfs, isolation) -> Result<()>` — execvp avec sandbox
4. `daedalus-stub` devient un shell binaire au-dessus de `daedalus-runtime`

**Bénéfice** : permet d'embedder le runtime dans un launcher custom, ou d'inspecter/patcher un `.ere` sans exécuter le stub.

### 6. Resource limits (cgroups) — 1 jour

**État** : le stub entre dans un mount namespace + seccomp. Les cgroups sont le complément naturel.

**Implémentation** :
1. Ajouter les flags `--cpu-limit`, `--memory-limit`, `--pid-limit`
2. Écrire dans `cgroup.procs` avant `execvp` (Linux-only, no-op sur macOS/Windows)
3. Nettoyage automatique du cgroup après exec

**Bénéfice** : argument enterprise. "Cette app a droit à 512MB RAM max, 1 CPU, 50 processus".

### 7. Post-extract hooks — 1-2 jours

**État** : le stub extrait le rootfs et execvp l'entrypoint. Rien ne permet de faire du setup avant l'entrypoint.

**Implémentation** :
1. Ajouter un fichier `.daedalus/hooks.json` dans le payload metadata section
2. Format :
   ```json
   {
     "pre": ["cp /etc/hosts /app/etc/hosts", "ln -s /app/data /data"],
     "post": ["curl -s https://health.check/ready"]
   }
   ```
3. Le stub exécute les hooks dans l'ordre avant/après l'entrypoint

**Bénéfice** : résout des use cases réels (migrate DB, warm cache, register service) sans modifier le code de l'app.

### 8. WASM/WASI runtime embedding — 3-5 jours

**État** : les flags `--wasm`, `--wasi`, `--component-model` existent dans `args.rs` mais sont `#[arg(hide = true)]` et non implémentés dans le stub.

**Implémentation** :
1. Embedder `wasmtime` dans le runtime
2. Ajouter un `wasm_executor.rs` dans le stub qui appelle `wasmtime::Store::new()` + `instance.export("_start")`
3. Activer les flags cachés

**Bénéfice** : différenciation vs PyInstaller/pkg/nexe. Personne ne fait du packaging WASM single-file avec sandbox + delta updates.

### 9. Registry serveur (daedalus serve) — 2-3 jours

**État** : `daedalus-core/src/registry.rs` implémente push/pull/list. `daedalus-cli` a `--publish` et `--registry`. Il manque le serveur.

**Implémentation** :
1. Serveur HTTP minimal : `/push`, `/pull/{hash}`, `/list`
2. Stockage filesystem ou S3
3. Auth basique (Bearer token)

**Bénéfice** : permet de partager des layers entre machines (ex: même Python runtime partagé entre 50 serveurs).

## Ce qui n'est PAS prioritaire

- ❌ OCI/WASM (`--to oci|appimage|wasm`) — **DEPRECATED** (dilution, voir §5)
- ❌ Monorepo Turborepo/Nx — nice-to-have, pas blocking pour MVP
- ❌ 50 repos GitHub trending — integration testing, pas un feature produit
- ❌ Edge/IoT OTA channels — SISR le couvre déjà (delta updates + signatures)
- ❌ IA agents / AI agent marketplace — DEGACER'd, le packaging universel couvre le besoin
- ❌ Docker-in-Daedalus — contredit la philosophie "no daemon"
- ❌ Plugin system avec scripting — les hooks JSON sont suffisants
- ❌ SDK Python/Node/Go avant d'avoir la demande utilisateur — `daedalus-runtime` d'abord

## §7 — MCU companion systems (edge/IoT)

### Limitation fondamentale

Un `.daedalus` est un ELF Linux (ou Mach-O / PE). Il ne peut **pas** s'exécuter directement
sur un MCU (ESP32, STM32, Arduino, etc.) : pas de noyau POSIX, pas de filesystem, RAM en Ko.
Ce n'est pas une question d'implémentation — c'est une limitation architecturale.

### Ce qui est possible : .ere comme composant d'un système MCU

| Cas | Setup | Use case | Valeur |
|---|---|---|---|
| Companion computer + MCU | Pi/Linux + ESP32/STM32 | Agent communication (MQTT/Modbus), agent IA edge, firmware config tool | Packaging + sandbox + delta updates pour le companion |
| Gateway IoT | Linux embarqué + MCUs via UART/I2C/SPI/CAN | Agent protocol bridging (Modbus→MQTT, CAN→MQTT), fleet monitoring, logging | Isolation sandbox entre MCUs, signing pour intégrité |
| Tools host-side | Laptop/serveur | esptool, openocd, fleet config, firmware audit/security scan | Sandbox pour outils dangereux, distro signée + delta updates |
| Edge appliance | Pi/NAS + MCUs intégrés | Monitoring agricole/industriel, agent collecte données | Sandbox par composant, updates incrémentaux |
| Security tooling | Host + MCU fleet | Firmware analysis, CVE detection, protocol audit | Distribution signée d'outils de pentest MCU (Phrack/BH Europe creds), sandbox isolation |

### Ce qui NÉCESSITERAIT une approche différente

| Besoin | Pourquoi ce n'est pas .ere | Alternative |
|---|---|---|
| Exécuter code sandboxé sur ESP32 | Pas de Linux, pas d'ELF | MicroPython / VM légère / microkernel |
| Packager firmware MCU dans .ere | Format userspace, pas bare-metal | Firmware image + metadata + checksum (format dédié) |
| Sandbox sur MCU | Pas de mécanismes comparables | MPU (memory protection unit), hardware isolation |

## §5 — Position B dépréciée (`--to family`)

Le `--to [oci|appimage|wasm]` est **déprécié**. Raisons :
- Le format `.daedalus` existant répond au besoin de distribution
- L'OCI n'est concurrent qu'au-delà du réseau (pas dans le segment desktop/local)
- WASM/AppImage = features hors segment prioritaire
- Le market value est dans la simplicité (single-file) + delta updates multi-runtime

## §6 — Dead code identifié (à nettoyer ou brancher)

- `ArtifactMetadata` : importé seulement par `metadata.rs` tests
- `LayerEncryption` dans `layer.rs` : défini mais jamais sérialisé dans le pipeline production

## Risques & mitigations

| Risque | Mitigation | Status |
|---|---|---|
| llamafile / Bun ajoute delta updates | Priorité : livrer universal binary + hot-swap | Priorité #1 |
| Cross-arch embedding complexe | CI matrix zigbuild + QEMU test local | Validé |
| layer.rs dead code = tech debt | Nettoyer (supprimer dead code non utilisé) | À faire |
| Pas de preuve de demande enterprise | Phase 0: consulting → collecter feedback clients | En cours |

## Stack technique (août 2026)

- **Rust 2021** stable, `opt-level="z"`, LTO, strip, `panic=abort`
- **stub** : musl static ELF, ~100KB, `unsafe` limité (FFI + seccomp BPF)
- **core/cli** : zero `unsafe`, ANSSI-Rust compliant
- **CI** : `cargo zigbuild` cross-compile, `cargo clippy -p {crate}`, `cargo test --workspace`
- **Tests** : 423 unit/integration/e2e, QEMU pour cross-arch validation
- **Cross-arch chaos monkey** : aarch64 (musl → qemu-aarch64-static) ✅, RISC-V (nécessite `riscv64-linux-gnu-gcc` — pas installé), Docker `--platform linux/arm64` ✅

## §8 — Refonte en crates publishables (post-MVP)

### Constat

`daedalus-cli` et `daedalus-stub` sont des binaires purs (pas de `[lib]`).
`daedalus-core` est la seule crate publishable aujourd'hui.

### Architecture cible

```
daedalus-core       ← déjà bon, pas toucher (format + crypto + SISR + layers)
daedalus-build      ← extrait la logique de build de daedalus-cli
daedalus-runtime    ← extrait le stub en lib (extraction + exec + sandbox)
daedalus-cli        ← shell CLI au-dessus de daedalus-build
daedalus-stub       ← shell binaire au-dessus de daedalus-runtime
daedalus-server     ← API HTTP au-dessus de daedalus-build (optionnel)
daedalus-py         ← PyO3 au-dessus de daedalus-build (optionnel)
```

### Principe de migration

Chaque étape est indépendante. Les binaires existants restent fonctionnels
à chaque étape. Pas de breaking change pour les users du CLI.

### Bénéfices

- `daedalus-build` : permet de builder des `.ere` depuis du code Rust (serveur CI,
  IDE plugin, pipeline custom) sans invoquer un binaire
- `daedalus-runtime` : permet d'embedder le runtime daedalus dans un launcher custom,
  ou d'inspecter/patcher un `.ere` sans exécuter le stub
- `daedalus-server` : déporte les builds cross-arch lourds sur une machine dédiée
- `daedalus-py` : `daedalus.build("myapp/")` depuis un setup.py ou Makefile

### Ce qu'on ne fait PAS

- Pas de `[lib]` ajouté aux binaires existants : ça mélange les responsabilités
- Pas de fusion cli+stub : cycles de vie différents, tailles différentes,
  contraintes de sécurité opposées
- Pas de SDK avant d'avoir la demande utilisateur
