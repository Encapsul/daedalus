# x.bin Roadmap: Features & Limitations

## Vision

x.bin transforme n'importe quelle application en un binaire ELF auto-extractible unique. Ce document liste les limitations actuelles et les features à implémenter pour en faire un outil de production à long terme.

---

# Master Action Plan — Todo List

Canonical, actionable task list. Every item below is a concrete change with
the files/crates it touches. Cross off as done; the detailed rationale for the
portability items lives in "Portabilité du stub" and "Blocants Critiques".

## Phase 0 — Security & correctness (do first, everywhere)

- [ ] **Isolation fail-open** — `isolation.parse().unwrap_or(1)` accepts any
      invalid `--isolation` value (audit CRITICAL). Reject with an error
      instead of defaulting to sandbox. (`xbin-cli/src/commands/build.rs`)
- [ ] **Predictable temp dir** — `/tmp/xbin-build-tools/node` is a fixed
      location (audit HIGH). Use `tempfile::tempdir()`.
      (`xbin-cli/src/commands/build.rs:1060`)
- [ ] **PATH injection** — `std::env::set_var("PATH", ...)` mutates the global
      environment (audit HIGH). Use `Command::env()`.
- [ ] **Panic leaks** — `human_panic` prints file/line on panic (audit MEDIUM).
      Hide source locations.
- [ ] **HKDF salt** — deterministic salt for key derivation (audit MEDIUM).
      Generate a random per-encryption salt. (`xbin-core/src/encrypt.rs`)
- [ ] **`DEFAULT_REGISTRY` placeholder** — `xbin.example.com` must not be a
      working default (audit MEDIUM). Make the registry URL mandatory.
- [ ] **Zeroization of keys** — add `zeroize` + `Zeroizing<T>` in
      `encrypt.rs`, `keygen.rs`, `sign.rs`. (audit HIGH-2)
- [ ] **`find_stub` stale-stub bug** — `find_stub` prefers `/usr/local/bin/xbin-stub`
      which can silently embed an obsolete stub (a 2026-07-27 stub with a
      relative-PATH bug was embedded this way). Prefer fresh builds
      (native `target/` then musl `target/`), warn if the embedded stub is
      older than N days. (`xbin-cli/src/commands/build.rs:1444`)

## Phase 1 — Port linux-arm64 (cheapest win, mostly done)

Status: **DONE** (2026-08-07). Verified end-to-end: aarch64 stub built via
`cargo zigbuild` (zig as cross cc+linker), hello-node packed with
`--target aarch64`, runs under `qemu-aarch64-static` → HTTP 200.

Key finding: **zstd-sys 2.0.x compiles `huf_decompress_amd64.S` (x86_64 asm)
unconditionally** (`build.rs:144`), which breaks every cross-arch link. Fixed
with a cfg-gated direct dep disabling asm on non-x86_64
(`stub/Cargo.toml`). Caveats: sandbox level 2 (`unshare` userns) fails under
QEMU user-mode — verify on real ARM hardware; node download for linux-arm64
worked.

- [x] **Cross toolchain** — `rustup target add aarch64-unknown-linux-musl`
      + `cargo-zigbuild` (zig as cc/linker wrapper; rustc passes GNU-ld flags
      that bare `zig cc` rejects).
- [x] **Build stub for arm64** — `cargo zigbuild -p xbin-stub --target aarch64-unknown-linux-musl --release`
      (`Makefile:14` `TARGET` var can't express the linker; zigbuild does).
- [x] **Verify node download** — linux-arm64 tarball path exists
      (`build.rs:1068-1073`); extraction works (arm64 node embedded).
- [x] **Verify stub lookup** — `find_stub` maps aarch64
      (`build.rs:1454`); auto-found `/tmp/xbin-stub-target/aarch64-…`.
- [x] **Metadata arch** — `resolve_arch` emits ARCH_AARCH64
      (`assembly.rs:33`); confirmed end-to-end.
- [x] **Smoke test arm64** — `xbin build --target aarch64` on hello-node, run
      under QEMU (`qemu-aarch64-static`), HTTP 200. (~45s cold under TCG;
      native ~2s.)
- [ ] **CI aarch64** — add aarch64 stub build + boot test to `release.yml`
      (use `cargo-zigbuild`; boot test on native ARM runner or QEMU, isolation
      level 1 for the QEMU leg).

## Phase 2 — Port macOS (Mach-O stub)

- [x] **Self-path** — `self_exe()` wrapper: readlink(`/proc/self/exe`) on
      Linux, `std::env::current_exe()` on macOS. Applied at all four sites
      (`run()`, `bin_path`, SISR update + remote). (`stub/src/main.rs:305`)
- [ ] **Cache dir** — use `dirs::cache_dir()` (already a dep,
      `stub/Cargo.toml:28`) → `~/Library/Caches/xbin/<hash>/rootfs/`.
      (`cache_dir()` currently hardcodes `~/.cache/xbin`.)
- [x] **cfg-gate Linux-only code** — `enter_userns`/`pivot_root_into`/
      `install_seccomp_denylist`/`write_proc` are `#[cfg(target_os = "linux")]`;
      the `use_pivot` blocks in `exec_app`/`supervise_services` and
      `enter_namespace_if_needed` are no-ops off-Linux; `mod landlock` is
      cfg-gated. macOS gets plain extract-to-cache + exec.
- [x] **exec model** — `execvp` via libc exists on macOS; unchanged.
- [ ] **Code-signing gotcha** — appending payload+footer invalidates a signed
      Mach-O. Either re-sign after assembly (`codesign`) or embed the payload
      in a Mach-O section. Implement the re-sign path and document it.
      (x.bin is meant for unsigned distribution, but notarization must not
      break.)
- [x] **darwin runtimes** — `ensure_node_download` maps `(os, arch)` →
      nodejs.org tarballs incl. `node-v<ver>-darwin-{arm64,x64}.tar.gz`
      (gzip via `flate2`); `ensure_node` downloads into a per-target cache
      dir and prepends to PATH so the embedded interpreter is the target
      node. (`build.rs:1058`)
- [x] **Stub build for darwin** — `cargo zigbuild --target
      {aarch64,x86_64}-apple-darwin` (zig libSystem stubs) produces both
      Mach-O stubs; `find_stub` maps darwin triples to the stub triple.
      Cross-build assembled end-to-end on the Linux builder (Mach-O arm64).
- [ ] **Sandbox** — seccomp/landlock have no macOS equivalent; evaluate
      Seatbelt (`sandbox_exec`) as optional hardening. Note as a documented
      gap in the security posture.
- [ ] **Smoke test macOS** — Actions `macos-14` runner: pack hello-node, run,
      HTTP 200.
- [ ] **Cross-target dependency install** — `npm install` cannot run a target
      node on a foreign host (Linux builder + darwin target). For target
      builds, either skip install for zero-dep apps (current behavior) or
      use `npm install --platform/--arch` for the target layout.

## Phase 3 — Port Windows (PE / EXE stub)

- [x] **Self-path** — `GetModuleFileNameW` (windows crate / winapi).
- [x] **Cache dir** — `%LOCALAPPDATA%\xbin\cache\<hash>\rootfs\`.
- [x] **No exec on Windows** — spawn the entrypoint child with `CreateProcess`
      and either wait for it (exit code passthrough) or detach. The stub
      cannot exec over itself.
- [x] **cfg-gate** — Linux-only modules (unshare/seccomp/landlock/pivot_root)
      compile out; new `#[cfg(target_os = "windows")]` module.
- [x] **win-x64 runtime** — `download_node` → `node-v<ver>-win-x64.zip`;
      extraction uses the `zip` crate; `node.exe`/`npm.cmd` staged in the
      tools dir and embedded as `usr/bin/node.exe` (cross-OS embed resolves
      the `.exe` from the target tools dir ahead of the host interpreter).
- [x] **Stub build** — `x86_64-pc-windows-gnu` / `-msvc` (CLI already builds
      for windows-gnu, ROADMAP §3).
- [x] **Output extension** — assemble to `app.exe` for PE targets.
- [ ] **Smoke test Windows** — Actions `windows-latest` runner: pack, run,
      HTTP 200.

## Phase 4 — Multi-target CLI (the enabler for "run everywhere")

- [x] **One `--target` syntax** — `parse_target` accepts Rust triples
      (`aarch64-apple-darwin`, `x86_64-unknown-linux-musl`) and short forms
      (`x86_64`|`aarch64`, defaulting to Linux). Wired into stub selection
      (`find_stub`), runtime download (`ensure_node`), and interpreter embed.
      Short OS aliases now include `linux-x64`/`linux-arm64`; remaining:
      `--cross-compile` comma list.
- [ ] **Multi-arch output** — `xbin build --target linux-x64,linux-arm64`
      emits one artifact per target.
- [x] **Per-target stub selection** — `find_stub` keyed by target maps to the
      per-(os,arch) stub triple (ELF musl / Mach-O darwin / PE windows).
- [x] **Per-target runtimes** — `ensure_node_download` matrix keyed by (os,
      arch) incl. darwin + windows (zip); embeds the runtime matching the
      target, not the builder's host.
- [x] **Per-platform file naming** — `.xbin` for ELF/Mach-O, `.exe` for PE.
- [ ] **Docs** — `xbin build --help` examples + README support matrix.

## Phase 5 — Benchmark-driven performance (see "Comparative benchmark")

- [ ] **Cold start < 500 ms** — current 2091 ms is single-threaded zstd
      extraction of the node runtime (warm start is 95 ms → launcher is fast).
      Use `zstdmt` multithreaded decompression (`zstd` crate with `zstdmt` is
      already a dev-dep; backhand supports zstd) or lower build compression.
- [ ] **On-disk footprint 122.6 MiB ≫ artifact 44 MiB** — cache GC
      (limitation #14), squashfs mount by default (v5 supported), cross-version
      dedup (limitation #11).
- [ ] **Pin builder Node version** — release pipeline embeds the builder's
      node; pin it for reproducible artifacts.
- [ ] **Re-run benchmark** after each perf change (`bash benchmarks/comparison/run.sh`).

## Phase 6 — CI / verification matrix

- [ ] **`release.yml` matrix** — linux x86_64 + aarch64 (QEMU boot test),
      macOS native, Windows native; each builds CLI + stub, packs hello-node,
      runs it.
- [ ] **Cross-platform verification loop** — per-platform
      `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --workspace`
      (per-crate clippy, per AGENTS.md).

## Phase 7 — Feature backlog (existing open items)

- [ ] **Electron desktop apps** — `Runtime::Electron` in `detect.rs`,
      `EmbeddedInterpreter` mapping, entrypoint `electron`,
      `--ignore-scripts`. (high priority, security)
- [ ] **RUN 2.6 remote build cache** (style Depot) — implementation in
      `paths.rs` incomplete.
- [ ] **RUN 4.1–4.6** — sandboxing + container-escape hardening.
- [ ] **RUN 5.1–5.6** — WASM runtime support + edge cases.
- [ ] **Remove `docs/planning/`** — still present (HANDOFF.md,
      xbin-project.md, .excalidraw, .pdf).
- [ ] **Symlinks for doc files** — ROADMAP/CODE_STYLE/RULES/CLAUDE/AGENTS as
      copies outside the repo + symlinks inside.

---


## État Actuel — Todo List (session ses_04c7)

### ✅ Terminé

- [x] RUN 1.1–1.5 + RUN 2.1–2.5 + RUN 3.1–3.6
- [x] atty removal, commande `publish`, refactor cache build, fix ldd musl, permissions vfat
- [x] EDGE EMBED : interpréteur statique (géré par `ldd_deps` → vec vide)
- [x] EDGE BUILD : permissions vfat préservées
- [x] EDGE BUILD : noms de fichiers non-UTF8 (via `to_string_lossy`)
- [x] EDGE PHP : détection Laravel Octane (RoadRunner) + embed `rr`
- [x] EDGE GO : app CGO — embed libc `.so` (via `ldd_deps`)
- [x] EDGE RUBY : gems natives (nokogiri, mysql2) — embed via scan `ldd`
- [x] EDGE DEPS : pip retry/timeout pour résilience proxy
- [x] EDGE NODE : skip `.pnpm store` (éviter la duplication)
- [x] EDGE EMBED : multiples versions PHP → contraintes de version dans `check_php_platform_reqs`
- [x] Fix sécurité : cron `@every` invalide → 3600s au lieu de 0 (plus de boucle CPU)
- [x] Fix sécurité : niveau de compression par défaut 3 au lieu de 19 (`python.rs`)
- [x] CT hardening : `verify_sha256` (stub) compare en temps constant (XOR
      bit-mask) au lieu de `!=` early-exit → pas de fuite de préfixe sur le
      digest SHA-256 (equivalent `subtle::ConstantTimeEq`, zero dépendance)
- [x] v4 `--encrypt` opérationnel : chiffrement AES-256-GCM du payload (clé
      dérivée de la seed Ed25519 via HKDF), métadonnées `crypto` (nonce/seed)
      dans le JSON v4, `FLAG_ENCRYPTED` + footer v4, et hash/signature qui
      couvrent le **ciphertext**. `--encrypt` exige `--key`.
- [x] Alignement `trust` path : la CLI écrit/lit `~/.xbin/trusted-keys` (ou
      `$XBIN_TRUSTED_DIR`), exactement comme le stub → corrige le
      "trusted keys directory not found" sur les binaires signés/chiffrés.
- [x] `hkdf_derive_key` (stub) : adapter au retour `Zeroizing<..>` du core
      après le refactor zeroize (déblocage compile).

### ⏳ En cours / À faire

- [ ] **EDGE NODE : détection apps desktop Electron** (haute priorité, sécurité)
      → ajouter `Runtime::Electron` dans `detect.rs`, mapping `EmbeddedInterpreter`, entrypoint `electron`, gestion `--ignore-scripts`
- [ ] **RUN 2.6 : cache build distant (style Depot)** — design/architecture terminés, **implémentation dans `paths.rs` non aboutie** (échec d'édition, JSON "Unterminated string")
- [ ] **RUN 4.1–4.6** : sandboxing + protection contre l'évasion de conteneur
- [ ] **RUN 5.1–5.6** : support runtime WebAssembly + edge cases
- [ ] **DESIGN : supprimer `docs/planning/`** (toujours présent : HANDOFF.md, xbin-project.md, .excalidraw, .pdf)
- [ ] **Zeroization des clés crypto** : `encrypt.rs`, `keygen.rs`, `sign.rs` — ajouter le crate `zeroize`, wrapper `Zeroizing<T>` (audit : HIGH-2)
- [ ] **Symlinks** : ROADMAP.md, CODE_STYLE.md, RULES.md, CLAUDE.md, AGENTS.md → copies hors repo + symlinks à l'intérieur
- [ ] **Stub DRY** : `stub/src/main.rs::trusted_keys_dir()` duplique
      `xbin_core::paths::trusted_keys_dir()` (identique pour l'instant).
      Factoriser pour appeler le core → édition du stub (security-critical,
      ask-first). Pas bloquant tant que les deux implémentations restent
      identiques.
- [ ] `--encrypt` + `--enable-sisr` : actuellement rejeté
      (`--encrypt is not supported together with --enable-sisr`). Implémenter
      le chunkage du ciphertext + déchiffrement SISR dans le stub.
- [ ] clig.dev : `xbin trust --json` pour une sortie machine-parseable
      (`verify`/`inspect` l'ont déjà ; `trust` non). Facultatif.

### 🔒 Correctifs sécurité restants (audit)

- [ ] CRITICAL : `isolation.parse().unwrap_or(1)` — rejeter les valeurs invalides au lieu de fail-open (build.rs)
- [ ] HIGH : temp dir prévisible `/tmp/xbin-build-tools/node` → `tempfile::tempdir()`
- [ ] HIGH : injection PATH via `set_var("PATH", ...)` → `Command::env()`
- [ ] MEDIUM : `human_panic` — masquer file/line dans les messages de panique
- [ ] MEDIUM : salt/info HKDF fixes → sel aléatoire par chiffrement
- [ ] MEDIUM : `DEFAULT_REGISTRY` placeholder `xbin.example.com` → config obligatoire

---

## Comparative benchmark (2026-08-07) — improvements revealed

Harness: `benchmarks/comparison/` (x.bin vs Docker / pkg / AppImage / Flatpak,
same zero-dep Node 24 app, machine profile recorded per run). Aggregated
results: `benchmarks/comparison/comparison.md`.

| Packager | Artifact | On-disk | Cold start | Warm start | Idle RSS | Host deps |
|----------|----------|---------|------------|------------|----------|-----------|
| **x.bin** | **44.0 MiB** | 122.6 MiB | 2091 ms | **95 ms** | 48.6 MiB | **none** |
| docker | 76.7 MiB | 76.7 MiB | 6823 ms | — | 49.9 MiB | docker-daemon |
| pkg | 70.2 MiB | 70.2 MiB | 99 ms | — | 55.0 MiB | none |
| appimage | 24.4 MiB | 24.4 MiB | 439 ms | — | 55.0 MiB | fuse-or-extract |
| flatpak | — | — | — | — | — | flatpak |

(Machine: Codespace 4 vCPU EPYC, 15 Gi RAM, overlayfs in a container.)

### Items to implement (by gain priority)

- [ ] **Cold start 2.1 s = single-threaded zstd extraction of the node runtime**
      (warm 95 ms proves the launcher is fast; the gap is extraction).
      → multithreaded decompression in the stub (crate `zstd`/zstdmt instead
      of single-thread `ruzstd`), or a lower build compression level.
      Goal: cold < 500 ms.
- [ ] **On-disk 122.6 MiB ≫ artifact 44 MiB** (decompressed rootfs).
      → cache GC (limitation #14), squashfs mount (v5, already supported),
      dedup between versions (limitation #11).
- [ ] **`find_stub` can silently embed an obsolete installed stub** (bug seen:
      /usr/local/bin stub dated 2026-07-27 with a relative-PATH bug was
      embedded). → prefer fresh builds (native + musl `target/`), warn if the
      embedded stub is older than N days.
- [ ] **Pin the builder's Node version** in the release pipeline (x.bin embeds
      the builder's node → artifact reproducibility).

---

### Blocants Critiques

#### 1. Linux-only (format ELF)
**Problème** : x.bin ne produit que des binaires ELF. Impossible de lancer sur macOS (Mach-O) ou Windows (PE).

**Impact** :
- Pas de support développeurs macOS
- Pas de support déploiement Windows
- Pas de cross-architecture (ARM ↔ x86)
- Limité aux serveurs Linux uniquement

**Comparaison** :
- Docker : supporte Linux, macOS, Windows
- Wasmer : supporte toutes les plateformes via Wasm
- Bun : supporte Linux, macOS, Windows

---

#### 2. Pas de mécanisme de mise à jour
**Problème** : Une fois construit, le binaire est immuable. Pas de delta updates, pas de patching binaire.

**Impact** :
- Chaque update = rebuild complet + redistribution
- Pas de mise à jour sécutive possible
- Pas de correction de vulnérabilités sans rebuild

**Comparaison** :
- Docker : `docker pull` pour les mises à jour
- AppImage : auto-update intégré
- Snap : mises à jour automatiques

---

#### 3. Pas de sandboxing
**Problème** : L'application tourne avec les permissions utilisateur complètes. Pas de seccomp, pas de capabilities Linux, pas de Landlock.

**Impact** :
- Sécurité limitée en production
- Pas d'isolation entre applications
- Risque d'exploitation de vulnérabilités

**Comparaison** :
- Docker : namespaces + cgroups + seccomp
- Wasmer : sandboxing Wasm par défaut
- Snap : confinement AppArmor

---

#### 4. Pas de configuration runtime
**Problème** : La configuration = variables d'environnement au build. Pas d'injection de config au runtime.

**Impact** :
- Rebuild pour changer une config
- Pas de config par environnement (dev/staging/prod)
- Pas de hot-reload

**Comparaison** :
- Docker : `-e` et `-v` pour l'injection
- Kubernetes : ConfigMaps et Secrets
- Bun : variables d'environnement au runtime

---

#### 5. Pas de gestion des secrets
**Problème** : Pas de vault, pas d'encryption des secrets. Secrets dans l'environnement = risque de leakage.

**Impact** :
- Secrets exposés dans les variables d'environnement
- Pas d'intégration avec les vaults existants
- Risque de secrets dans les logs

**Comparaison** :
- Docker : Docker secrets, HashiCorp Vault
- Kubernetes : Secrets natifs
- AWS : Secrets Manager

---

#### 6. Pas de stockage persistant
**Problème** : Cache = `~/.cache/xbin/<hash>/rootfs/`. Pas de volumes, pas de persistence entre les runs.

**Impact** :
- État perdu à chaque exécution
- Pas de bases de données persistantes
- Pas de fichiers de configuration persistants

**Comparaison** :
- Docker : volumes et bind mounts
- Kubernetes : PersistentVolumes
- LXC : stockage persistant

---

#### 7. Pas d'observabilité
**Problème** : Pas de logging intégré, pas de métriques, pas de tracing.

**Impact** :
- Difficile à monitorer en production
- Pas de debugging facilité
- Pas d'intégration avec les outils de monitoring

**Comparaison** :
- Docker : logging drivers, métriques
- Kubernetes : stdout/stderr → collecteurs
- Bun : console.log structuré

---

#### 8. Pas d'isolation réseau
**Problème** : Pas de namespaces réseau, pas de proxy, pas de load balancing.

**Impact** :
- Sécurité réseau limitée
- Pas d'isolation entre services
- Pas de control du trafic

**Comparaison** :
- Docker : network namespaces, overlay networks
- Kubernetes : NetworkPolicies
- Istio : service mesh

---

### Limitations Importantes

#### 9. Pas de limites de ressources
**Problème** : Pas de cgroups pour CPU/mémoire/PID.

**Impact** : Une application peut consommer toutes les ressources du système.

#### 10. Pas de health checks
**Problème** : Pas de mécanisme de vérification de santé intégré.

**Impact** : Difficile de savoir si l'application fonctionne correctement.

#### 11. Pas de layer caching
**Problème** : Les dépendances sont rebuild à chaque fois.

**Impact** : Builds lents pour les grandes applications.

#### 12. Pas de builds reproductibles
**Problème** : Le processus de build n'est pas déterministe.

**Impact** : Même source → binaires différents.

#### 13. Pas de mécanisme de rollback
**Problème** : Impossible de revenir à une version précédente.

**Impact** : Pas de récupération après une mise à jour ratée.

#### 14. Pas de garbage collection
**Problème** : Les anciennes versions s'accumulent.

**Impact** : Espace disque gaspillé.

#### 15. Pas de support WebAssembly
**Problème** : Limité aux binaires natifs.

**Impact** : Pas de portabilité universelle.

---

### Limitations Mineures

#### 16. Pas de package registry
**Problème** : Pas de dépôt central pour la distribution.

#### 17. Pas d'intégration desktop
**Problème** : Pas de fichiers .desktop/icônes pour les apps GUI.

#### 18. Pas d'auto-update
**Problème** : Pas de mise à jour automatique des binaires.

#### 19. Pas d'orchestration multi-conteneurs
**Problème** : Support service unique uniquement.

---

## Portabilité du stub (items à gérer)

Problèmes connus qui contraignent une release multi-plateforme complète.
Chaque entrée décrit le symptôme, l'état actuel et ce qu'il faut pour fermer.

### 1. Le stub ne compile pas sur riscv64 et ppc64le

**Symptôme** : `xbin-stub` échoue à compiler sur `riscv64gc-unknown-linux-musl`
et `powerpc64le-unknown-linux-musl` avec 21 erreurs `E0425` (constantes
seccomp manquantes : `AUDIT_ARCH`, `LD_PATHS`, `SYS_PTRACE`, `SYS_MOUNT`,
`SYS_PIVOT_ROOT`, `SYS_KEXEC_*`, `SYS_INIT_MODULE`, …).

**Cause** : `stub/src/main.rs` ne définit les constantes du filtre seccomp
que pour `x86_64`, `aarch64`, `x86` (i686) et `arm`. Le filtre est
security-critical (syscalls ptrace/mount/pivot_root/…) et les numéros de
syscalls riscv64 (asm-generic) et ppc64le (arch/powerpc) diffèrent ; ils ne
sont pas encore câblés, faute de pouvoir les vérifier.

**Décision actuelle** : riscv64 et ppc64le sont publiés **CLI-only** dans le
pipeline de release — le stub n'y est ni construit ni empaqueté. Le sandbox
seccomp reste complet sur les 4 archs supportés.

**Pour fermer** (options, à trancher) :
1. Câbler les constantes riscv64/ppc64le en les vérifiant depuis les headers
   kernel (`asm-generic/unistd.h`, `arch/powerpc/include/uapi/asm/unistd.h`)
   et valider le filtre au runtime sur vraie machine/QEMU.
2. Compile-time gate : désactiver seccomp sur les archs non supportés en
   gardant landlock + namespaces (dégradation assumée du poste de sécurité).
3. Laisser ces targets en CLI-only définitivement.

### 2. s390x : pas de build possible

`s390x-unknown-linux-musl` : pas de prébuild `rustup` (aucune libstd) et le
CLI échoue aussi (5 erreurs, dépendances sans support s390x). Target à
exclure ou à retenter quand un prébuild existe.

### 3. Windows

Le CLI compile pour `x86_64-pc-windows-gnu` et `i686-pc-windows-gnu`
(`xbin.exe`, tar.rs porté). Le stub est Linux-only (unshare/pivot_root/
seccomp/landlock) — pas de port Windows prévu. Les archives Windows ne
contiennent que `xbin.exe`.

### 4. BSD (FreeBSD/NetBSD/OpenBSD)

Cross-compilation via zigbuild impossible (link fail : libs systèmes manquantes
via zig ; pas de std prébuildé rustup pour NetBSD/OpenBSD). Nécessite des
VMs natives dans le CI (`vmactions/freebsd-vm`, `netbsd-vm`, `openbsd-vm`).
À ajouter au release.yml.

---

## Features à Implémenter

### Priorité 1 : Critique (Haute)

#### 1. Support Cross-Platform
**Objectif** : Supporter macOS (Mach-O) et Windows (PE)

**Pourquoi c'est important** :
- Élargir l'audience aux développeurs macOS
- Permettre le déploiement Windows
- Rendre x.bin universel

**Comment** :
- Abstraire le code spécifique ELF dans le stub
- Créer des loaders spécifiques à chaque plateforme
- Utiliser la compilation conditionnelle Rust
- Tester sur toutes les plateformes

**Complexité** : 3-4 semaines

---

#### 2. Mises à Jour Delta
**Objectif** : Patching binaire pour éviter les rebuilds complets

**Pourquoi c'est important** :
- Réduire le temps de mise à jour
- Économiser la bande passante
- Permettre les mises à jour sécures

**Comment** :
- Utiliser bsdiff/bspatch pour le diffing binaire
- Stocker les patches alongside les binaires
- Implémenter l'application de patches dans le stub
- Ajouter les métadonnées de version au footer

**Complexité** : 2-3 semaines

---

#### 3. Injection de Configuration Runtime
**Objectif** : Injecter la config sans rebuild

**Pourquoi c'est important** :
- Config par environnement (dev/staging/prod)
- Hot-reload sans redémarrage
- Séparation code/config

**Comment** :
- Supporter les fichiers de config dans /etc/xbin/ ou ~/.config/xbin/
- Override par variables d'environnement au runtime
- Embedding de config avec lazy loading
- Support hot-reload

**Complexité** : 1-2 semaines

---

#### 4. Gestion des Secrets
**Objectif** : Gestion sécurisée des secrets

**Pourquoi c'est important** :
- Sécurité en production
- Intégration avec les vaults existants
- Éviter les secrets dans les logs

**Comment** :
- Intégrer avec le keyring système
- Supporter HashiCorp Vault, AWS Secrets Manager
- Encrypter les secrets at rest dans le binaire
- Décryption runtime avec injection de clés

**Complexité** : 2-3 semaines

---

#### 5. Stockage Persistant
**Objectif** : Volume mounts qui survivent entre les runs

**Pourquoi c'est important** :
- Persistance des données
- Bases de données persistantes
- Fichiers de config persistants

**Comment** :
- Supporter les volume mounts via flags CLI
- Répertoires persistants dans ~/.local/share/xbin/
- Bind mounts pour les répertoires hôte
- Gestion des quotas de stockage

**Complexité** : 1-2 semaines

---

#### 6. Observabilité
**Objectif** : Logging, métriques, tracing intégrés

**Pourquoi c'est important** :
- Monitoring en production
- Debugging facilité
- Intégration avec les outils existants

**Comment** :
- Logging structuré (format JSON)
- Export de métriques (format Prometheus)
- Distributed tracing (OpenTelemetry)
- Niveaux de log configurables

**Complexité** : 2-3 semaines

---

#### 7. Sandboxing
**Objectif** : Isolation de sécurité pour les applications

**Pourquoi c'est important** :
- Sécurité en production
- Isolation entre applications
- Réduction de la surface d'attaque

**Comment** :
- Filtres seccomp pour le filtrage de syscalls
- Drop des Linux capabilities
- Landlock pour le contrôle d'accès filesystem
- Profils AppArmor/SELinux

**Complexité** : 3-4 semaines

---

#### 8. Isolation Réseau
**Objectif** : Isolation par namespace réseau

**Pourquoi c'est important** :
- Sécurité réseau
- Isolation entre services
- Control du trafic

**Comment** :
- Créer des namespaces réseau
- Paires ethernet virtuelles
- Support proxy (HTTP/SOCKS)
- Configuration DNS

**Complexité** : 2-3 semaines

---

### Priorité 2 : Importante (Moyenne)

#### 9. Limites de Ressources
**Objectif** : cgroups pour CPU/mémoire/PID

**Complexité** : 1-2 semaines

#### 10. Health Checks
**Objectif** : Mécanisme de vérification de santé intégré

**Complexité** : 1 semaine

#### 11. Layer Caching
**Objectif** : Cache des dépendances pour accélérer les builds

**Complexité** : 2-3 semaines

#### 12. Builds Reproductibles
**Objectif** : Processus de build déterministe

**Complexité** : 1-2 semaines

#### 13. Mécanisme de Rollback
**Objectif** : Revenir à une version précédente

**Complexité** : 1-2 semaines

#### 14. Garbage Collection
**Objectif** : Nettoyage automatique des anciennes versions

**Complexité** : 1 semaine

#### 15. Support WebAssembly
**Objectif** : Compiler les apps en Wasm pour la portabilité

**Complexité** : 4-6 semaines

---

### Priorité 3 : Nice to Have (Basse)

#### 16. Package Registry
**Objectif** : Dépôt central pour la distribution

**Complexité** : 6-8 semaines

#### 17. Intégration Desktop
**Objectif** : Fichiers .desktop/icônes pour les apps GUI

**Complexité** : 1-2 semaines

#### 18. Auto-Update
**Objectif** : Binaires auto-mis à jour

**Complexité** : 2-3 semaines

#### 19. Orchestration Multi-Conteneurs
**Objectif** : Support multi-services

**Complexité** : 8-10 semaines

---

## Calendrier d'Implémentation

### Phase 1 : Core (Mois 1-3)
- Injection de config runtime
- Stockage persistant
- Observabilité
- Limites de ressources

### Phase 2 : Sécurité (Mois 4-6)
- Sandboxing
- Isolation réseau
- Gestion des secrets

### Phase 3 : Distribution (Mois 7-9)
- Mises à jour delta
- Mécanisme de rollback
- Garbage collection

### Phase 4 : Portabilité (Mois 10-12)
- Support cross-platform
- Support WebAssembly

### Phase 5 : Écosystème (Année 2+)
- Package registry
- Auto-update
- Orchestration multi-conteneurs

---

## Dette Technique

### Problèmes Actuels
1. Pas d'intégration tests pour cross-compilation
2. Pas de benchmarks pour les nouvelles features
3. Lacunes documentation pour les nouvelles features

### Refactoring Nécessaire
1. Abstraire le code spécifique plateforme
2. Moduler le stub launcher
3. Améliorer la gestion d'erreurs
4. Ajouter des tests complètes

---

## Métriques de Succès

### Performance
- Temps de build : < 30 secondes pour une app typique
- Temps de démarrage : < 100ms
- Overhead mémoire : < 10MB
- Taille du binaire : < 5MB

### Sécurité
- Zéro vulnérabilité critique
- Conformité ANSSI-Rust
- Couverture sandboxing : 100%
- Encryption des secrets : 100%

### Compatibilité
- Linux : x86_64, aarch64
- macOS : x86_64, arm64
- Windows : x86_64

### Adoption
- 1000+ utilisateurs actifs mensuels
- 100+ packages dans le registry
- 10+ contributeurs
