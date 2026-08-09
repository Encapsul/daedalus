# x.bin Roadmap: Features & Limitations

## Vision

x.bin transforme n'importe quelle application en un binaire ELF auto-extractible unique. Ce document liste les limitations actuelles et les features à implémenter pour en faire un outil de production à long terme.

---

# Master Action Plan — Todo List

Canonical, actionable task list. Every item below is a concrete change with
the files/crates it touches. Cross off as done; the detailed rationale for the
portability items lives in "Portabilité du stub" and "Blocants Critiques".

## Phase 0 — Security & correctness (do first, everywhere)

- [x] **Isolation fail-open** — `isolation.parse().unwrap_or(1)` accepts any
      invalid `--isolation` value (audit CRITICAL). Reject with an error
      instead of defaulting to sandbox. (`xbin-cli/src/commands/build.rs`)
- [x] **Predictable temp dir** — `/tmp/xbin-build-tools/node` is a fixed
      location (audit HIGH). Use `tempfile::tempdir()`.
      (`xbin-cli/src/commands/build.rs:1060`)
- [x] **PATH injection** — `std::env::set_var("PATH", ...)` mutates the global
      environment (audit HIGH). Use `Command::env()`.
- [x] **Panic leaks** — `human_panic` prints file/line on panic (audit MEDIUM).
      Hide source locations.
- [x] **HKDF salt** — deterministic salt for key derivation (audit MEDIUM).
      Generate a random per-encryption salt. (`xbin-core/src/encrypt.rs`)
- [x] **`DEFAULT_REGISTRY` placeholder** — `xbin.example.com` must not be a
      working default (audit MEDIUM). Make the registry URL mandatory.
- [x] **Zeroization of keys** — add `zeroize` + `Zeroizing<T>` in
      `encrypt.rs`, `keygen.rs`, `sign.rs`. (audit HIGH-2)
- [x] **`find_stub` stale-stub bug** — `find_stub` prefers `/usr/local/bin/xbin-stub`
      which can silently embed an obsolete stub (a 2026-07-27 stub with a
      relative-PATH bug was embedded). Prefer fresh builds
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
- [x] **CI aarch64** — add aarch64 stub build + boot test to `release.yml`
      (use `cargo-zigbuild`; boot test on native ARM runner or QEMU, isolation
      level 1 for the QEMU leg).

## Phase 2 — Port macOS (Mach-O stub)

- [x] **Self-path** — `self_exe()` wrapper: readlink(`/proc/self/exe`) on
      Linux, `std::env::current_exe()` on macOS. Applied at all four sites
      (`run()`, `bin_path`, SISR update + remote). (`stub/src/main.rs:305`)
- [x] **Cache dir** — `dirs::cache_dir()` returns `~/Library/Caches/xbin` on
      macOS; `xbin-core/src/paths.rs` has been updated to use `LOCALAPPDATA` on
      Windows and `XDG_CACHE_HOME`/`~/.cache` on Linux. (`cache_dir()` now
      handles all three platforms.)
- [x] **cfg-gate Linux-only code** — `enter_userns`/`pivot_root_into`/
      `install_seccomp_denylist`/`write_proc` are `#[cfg(target_os = "linux")]`;
      the `use_pivot` blocks in `exec_app`/`supervise_services` and
      `enter_namespace_if_needed` are no-ops off-Linux; `mod landlock` is
      cfg-gated. macOS gets plain extract-to-cache + exec.
- [x] **exec model** — `execvp` via libc exists on macOS; unchanged.
- [x] **Code-signing gotcha** — re-sign after assembly using `codesign` in
      `xbin-cli/src/commands/build.rs:sign_macos_binary()`. Ad-hoc signing by
      default; distribution uses `XBIN_CODESIGN_IDENTITY` env var.
- [x] **darwin runtimes** — `ensure_node_download` maps `(os, arch)` →
      nodejs.org tarballs incl. `node-v<ver>-darwin-{arm64,x64}.tar.gz`
      (gzip via `flate2`); `ensure_node` downloads into a per-target cache
      dir and prepends to PATH so the embedded interpreter is the target
      node. (`build.rs:1058`)
- [x] **Stub build for darwin** — `cargo zigbuild --target
      {aarch64,x86_64}-apple-darwin` (zig libSystem stubs) produces both
      Mach-O stubs; `find_stub` maps darwin triples to the stub triple.
      Cross-build assembled end-to-end on the Linux builder (Mach-O arm64).
- [x] **Sandbox** — Seatbelt (`sandbox-exec`) evaluated as optional hardening.
      `stub/src/macos_sandbox.rs` provides a baseline profile restricting
      filesystem access to the rootfs and xbin cache. Applied when
      `meta.landlock` is true on macOS.
- [x] **Smoke test macOS** — CI job added to `.github/workflows/ci.yml`
      (`macos` runner: `macos-latest`, builds CLI, runs hello-web smoke test).
- [x] **Cross-target dependency install** — `npm install` cannot run a target
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
- [x] **Smoke test Windows** — Actions `windows-latest` runner: pack, run,
      HTTP 200.

## Phase 4 — Multi-target CLI (the enabler for "run everywhere")

- [x] **One `--target` syntax** — `parse_target` accepts Rust triples
      (`aarch64-apple-darwin`, `x86_64-unknown-linux-musl`) and short forms
      (`x86_64`|`aarch64`, defaulting to Linux). Wired into stub selection
      (`find_stub`), runtime download (`ensure_node`), and interpreter embed.
      Short OS aliases now include `linux-x64`/`linux-arm64`; remaining:
      `--cross-compile` comma list.
- [x] **Multi-arch output** — `xbin build --target linux-x64,linux-arm64`
      emits one artifact per target (see `output_paths()` in `build.rs`).
- [x] **Per-target stub selection** — `find_stub` keyed by target maps to the
      per-(os,arch) stub triple (ELF musl / Mach-O darwin / PE windows).
- [x] **Per-target runtimes** — `ensure_node_download` matrix keyed by (os,
      arch) incl. darwin + windows (zip); embeds the runtime matching the
      target, not the builder's host.
- [x] **Per-platform file naming** — `.xbin` for ELF/Mach-O, `.exe` for PE.
- [x] **Docs** — `xbin build --help` examples + README support matrix.

## Phase 5 — Benchmark-driven performance (see "Comparative benchmark")

- [ ] **Cold start < 500 ms** — current ~2142 ms is single-threaded zstd
   extraction of the node runtime (warm start is 96 ms → launcher is fast).
  The `zstd` crate's `zstdmt` feature only enables multithreaded *compression*;
  the streaming `Decoder` has no multithreaded decompression API in 0.13.
  Options: (a) use `zstd-safe` raw API with `ZSTD_DCtx_setParameter` +
  `ZSTD_c_nbWorkers` equivalent if exposed in future zstd releases, (b) spawn
  N threads each decompressing a slice of the stream manually, (c) lower
  build compression level for faster decompression at the cost of larger
  artifacts.
- [ ] **On-disk footprint 122.6 MiB ≫ artifact 44 MiB** — cache GC
  (limitation #14), squashfs mount by default (v5 supported), cross-version
  dedup (limitation #11).
- [x] **Pin builder Node version** — release pipeline embeds the builder's
      node; pin it for reproducible artifacts. Done via `XBIN_NODE_VERSION`
      env var in `release.yml` + `build.rs:1608`.
- [ ] **Re-run benchmark** after each perf change (`bash benchmarks/comparison/run.sh`).

## Phase 6 — CI / verification matrix

- [x] **`release.yml` matrix** — linux x86_64 + aarch64 (QEMU boot test),
      macOS native, Windows native; each builds CLI + stub, packs hello-node,
      runs it. (`release.yml` includes linux matrix with zigbuild, darwin-amd64,
      darwin-arm64, windows-amd64, QEMU aarch64 boot test).
- [x] **Cross-platform verification loop** — per-platform
      `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --workspace`
      (per-crate clippy, per AGENTS.md). Verified in CI jobs: rust, macos, windows.

## Phase 7 — Feature backlog (existing open items)

- [x] **Electron desktop apps** — `Runtime::Electron` in `detect.rs`,
      `EmbeddedInterpreter::Electron` mapping, entrypoint `electron`
      (main.js/main.ts/index.js/index.ts or package.json with electron dep).
      Detection priority: Electron beats Node when main file present.
      (`xbin-core/src/detect.rs`, `xbin-core/src/metadata.rs`)
- [x] **`--encrypt` + `--enable-sisr`** — per-chunk AES-256-GCM encryption
      (`encrypt_chunks`/`decrypt_chunks` in `xbin-core/src/encrypt.rs`). Each
      SISR chunk gets an independent HKDF-derived key; manifest tracks
      ciphertext hashes; stub decrypts each chunk independently before SISR
      extraction. Build guard removed; chunked decrypt wired into stub `run()`.
- [x] **`trusted_keys_dir` DRY** — `stub/src/main.rs::trusted_keys_dir()` now
      delegates to `xbin_core::paths::trusted_keys_dir()`; single source of
      truth for `$XBIN_TRUSTED_DIR` / `~/.xbin/trusted-keys` resolution.
- [x] **WASM runtime** — `Runtime::Wasm` detection (`index.wasm`, `app.wasm`,
      `main.wasm`, `.wasm` extension) + entrypoint `wasmtime /app/<entry>` in
      `xbin-core/src/detect.rs`. Tests added.
- [x] **RUN 2.6 remote build cache** (style Depot) — `RemoteCacheBackend` trait
      in `xbin-core/src/paths.rs`, `FsRemoteCache` + `HttpRemoteCache` backend
      (`xbin-cli/src/remote_cache.rs`), `RemoteBuildCache` facade with
      remote-first find + dual store. CLI flags: `--remote-cache-url`,
      `--remote-cache-max-entries`.
- [x] **RUN 4.1–4.6** — sandboxing + container-escape hardening.
      `running_in_container()` detection (dockerenv/containerenv/cgroup) with
      warning when `pivot_root` is requested. `gc_extraction_cache()` bounds
      on-disk footprint (LRU, 16 entries).
- [x] **RUN 5.1–5.6** — WASM runtime edge cases (WASI, component model).
      `WasmConfig` now has `wasi` and `component_model` bools; CLI flags
      `--wasi` and `--component-model` inject `--wasi`/`--component-model`
      into the wasmtime argv.
- [x] **Remove `docs/planning/`** — deleted; symlinks in `docs/` point to root.
- [x] **Symlinks for doc files** — ROADMAP/CODE_STYLE/RULES/CLAUDE/AGENTS as
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

- [x] **EDGE NODE : détection apps desktop Electron** — `Runtime::Electron`,
      `EmbeddedInterpreter::Electron`, entrypoint `electron`, detection
      priority over Node when main file present.
- [x] **RUN 2.6 : cache build distant (style Depot)** — `RemoteCacheBackend`
      trait, `FsRemoteCache`, `HttpRemoteCache`, `RemoteBuildCache` facade,
      CLI flags `--remote-cache-url`/`--remote-cache-max-entries`.
- [x] **RUN 4.1–4.6** : sandboxing + protection contre l'évasion de conteneur —
      `running_in_container()` + warning, `gc_extraction_cache()`.
- [x] **RUN 5.1–5.6** : support runtime WebAssembly + edge cases —
      `--wasi`/`--component-model` flags, `WasmConfig` fields.
- [x] **DESIGN : supprimer `docs/planning/`** — supprimé.
- [x] **Symlinks** : ROADMAP.md, CODE_STYLE.md, RULES.md, CLAUDE.md, AGENTS.md
      → copies hors repo + symlinks à l'intérieur.
- [x] **Stub DRY** : `trusted_keys_dir()` délègue à `xbin_core::paths::trusted_keys_dir()`.
- [x] **`--encrypt` + `--enable-sisr`** — per-chunk AES-256-GCM, build guard
      supprimé, chunked decrypt dans le stub.
- [x] **clig.dev : `xbin trust --json`** — sortie machine-parseable.

### 🔒 Correctifs sécurité Phase 0 — résolus (audit)

- [x] CRITICAL : `isolation.parse().unwrap_or(1)` → `parse_isolation` fail-closed (`bail!` + `.with_context()?`, build.rs:17) ✅ clippy+tests
- [x] HIGH : temp dir prévisible `/tmp/xbin-build-tools/node` → `tempfile::tempdir()` (build.rs:729) ✅ clippy+tests
- [x] HIGH : injection PATH via `set_var("PATH", ...)` → `Command::env()` ✅ clippy+tests
- [x] MEDIUM : `human_panic` — `setup_panic!()` cache les file/line du message stdout ✅ (nota: le rapport disque `~/.cache/.../*.panic` conserve la backtrace → hardening optionnel)
- [x] MEDIUM : salt/info HKDF fixes → sel aléatoire par chiffrement (`encryption_salt_hex` dans CryptoMeta) ✅ vérifié e2e (v4 --encrypt, sel random, decrypt stub sans env)
- [x] MEDIUM : `DEFAULT_REGISTRY` placeholder `xbin.example.com` → `Option<Registry>` + `XBIN_REGISTRY` fail-closed ✅
- [x] **Nit non critique** : `scan.rs:227` garde `meta.get("isolation").unwrap_or(1)` — mais il s'agit du `xbin scan` (rapport JSON d'un binaire existant, default level 1 pour les binaires legacy), **pas** du `isolation.parse()` d'entrée utilisateur. Ne force pas le sandbox à l'exécution. Laissé tel quel (changement de sortie `scan` → à valider) ; optionnel.

---

## Comparative benchmark (2026-08-07) — improvements revealed

Harness: `benchmarks/comparison/` (x.bin vs Docker / pkg / AppImage / Flatpak,
same zero-dep Node 24 app, machine profile recorded per run). Aggregated
results: `benchmarks/comparison/comparison.md`.

| Packager | Artifact | On-disk | Cold start | Warm start | Idle RSS | Host deps |
|----------|----------|---------|------------|------------|----------|-----------|
| **x.bin** | **44.0 MiB** | 122.6 MiB | 2142 ms | **96 ms** | 48.6 MiB | **none** |
| docker | 76.7 MiB | 76.7 MiB | 6147 ms | — | 50.0 MiB | docker-daemon |
| pkg | 70.2 MiB | 70.2 MiB | 109 ms | — | 55.0 MiB | none |
| appimage | 24.4 MiB | 24.4 MiB | 29 ms | — | 55.6 MiB | fuse-or-extract |
| flatpak | — | — | — | — | — | flatpak |

(Machine: Codespace 4 vCPU EPYC, 15 Gi RAM, overlayfs in a container.)

### Items to implement (by gain priority)

- [ ] **Cold start ~2.1 s = single-threaded zstd extraction of the node runtime**
      (warm 96 ms proves the launcher is fast; the gap is extraction).
      → multithreaded decompression in the stub (crate `zstd`/zstdmt instead
      of single-thread `ruzstd`), or a lower build compression level.
      Goal: cold < 500 ms.
- [ ] **On-disk 122.6 MiB ≫ artifact 44 MiB** (decompressed rootfs).
      → cache GC (limitation #14), squashfs mount (v5, already supported),
      dedup between versions (limitation #11).
- [x] **`find_stub` can silently embed an obsolete installed stub** (bug seen:
      /usr/local/bin stub dated 2026-07-27 with a relative-PATH bug was
      embedded). Fix: removed `/usr/local/bin/xbin-stub` fallback entirely;
      fresh builds preferred, `which::which` only as last resort with warning.
- [x] **Pin the builder's Node version** in the release pipeline (x.bin embeds
      the builder's node → artifact reproducibility). Done via
      `XBIN_NODE_VERSION` env var in `release.yml` + `build.rs:1608`.

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

## Diagramme d'Architecture (ASCII — à redessiner dans Excalidraw)

```
                    ┌─────────────────────────────────────────────┐
                    │              DEVELOPPEUR                     │
                    │  xbin build ./myapp -o myapp.xbin            │
                    └──────────────────┬──────────────────────────┘
                                       │
                    ┌─────────────────┴────────────────────────────┐
                    │              xbin-cli (v0.5.0)                │
                    │  [build.rs]  ───►  detect_runtime()           │
                    │                 ───►  resolve_entrypoint()   │
                    │                 ───►  build_meta_json()      │
                    │                 ───►  embed_interpreter()    │
                    │                 ───►  create_tar_zstd()       │
                    │                 ───►  encrypt_payload()      │
                    │                 ───►  assemble_xbin()         │
                    │                 ───►  sign_file()             │
                    └────────────┬────────────────┬───────────────┘
                                 │                │
           ┌─────────────────────┘                └──────────────────┐
           │                                                          │
┌──────────▼──────────┐                                   ┌──────────▼──────────┐
│   xbin-core (v0.5.0)  │                                 │     xbin-stub         │
│  (bibliothèque Rust)  │                                 │  musl x86_64 / arm64   │
│                       │                                 │  Mach-O darwin         │
│  ┌─────────────────┐  │                                 │  PE windows            │
│  │ format.rs       │  │                                 │  (ELF auto-extractible)│
│  │  - Footer       │  │                                 └────────────────────────┘
│  │  - MAGIC=0xCAFE │  │                                             │
│  │  - v2..v5       │  │                                             │  /proc/self/exe
│  └─────────────────┘  │                                             ▼
│  ┌─────────────────┐  │                                ┌─────────────────────┐
│  │ tar.rs          │  │                                │   STUB (launcher)   │
│  │  - zstd+tar     │  │                                │                     │
│  │  - deterministic│  │                                │ 1. read_from()      │
│  └─────────────────┘  │                                │ 2. SHA-256 verify   │
│  ┌─────────────────┐  │                                │ 3. Ed25519 verify   │
│  │ encrypt.rs      │  │                                │ 4. AES-256-GCM      │
│  │  - HKDF         │  │                                │    decrypt (v4)      │
│  │  - AES-GCM      │  │                                │ 5. extract payload  │
│  │  - zeroize     │  │                                │ 6. enter namespace  │
│  └─────────────────┘  │                                │ 7. pivot_root       │
│  ┌─────────────────┐  │                                │ 8. seccomp BPF      │
│  │ detect.rs       │  │                                │ 9. landlock         │
│  │  - 13 runtimes  │  │                                │ 10. exec_app()      │
│  │  - entrypoint   │  │                                │     python3 /app/app.py
│  └─────────────────┘  │                                │     node /app/index.js
│  ┌─────────────────┐  │                                │     /app/myapp (go)
│  │ sisr/           │  │                                └─────────────────────┘
│  │  engine.rs      │  │                                             │
│  │  FastCDC chunk  │  │                                 execvp() → app
│  │  Merkle tree    │  │                                (python/node/etc.)
│  │  delta update   │  │                                             │
│  └─────────────────┘  │                                ┌───────────────┐
│  ┌─────────────────┐  │                                │  ~/.cache/    │
│  │ metadata.rs     │  │                                │  xbin/<hash>/ │
│  │  BunFeatures    │  │                                │  rootfs/      │
│  └─────────────────┘  │                                └───────────────┘
│  ┌─────────────────┐  │
│  │ assembly.rs     │  │
│  │  - build_meta   │  │
│  │  - assemble_xbin│  │
│  └─────────────────┘  │
│  ┌─────────────────┐  │
│  │ sisr_header.rs  │  │
│  │  - SisrFooterExt│  │
│  └─────────────────┘  │
│  ┌─────────────────┐  │
│  │ sisr_stage.rs   │  │
│  │  - build_artif. │  │
│  │  - sign/verify  │  │
│  └─────────────────┘  │
│  ┌─────────────────┐  │
│  │ manifest.rs      │  │
│  │  - DeltaManifest│  │
│  │  - ChunkEntry   │  │
│  └─────────────────┘  │
│  ┌─────────────────┐  │
│  │ chunker.rs      │  │
│  │  - FastCDC      │  │
│  └─────────────────┘  │
│  ┌─────────────────┐  │
│  │ compress.rs     │  │
│  │  - zstd         │  │
│  └─────────────────┘  │
│  ┌─────────────────┐  │
│  │ paths.rs        │  │
│  │  - cache_dir    │  │
│  │  - trusted_keys │  │
│  └─────────────────┘  │
│  ┌─────────────────┐  │
│  │ pkgmgr.rs       │  │
│  │  - npm/npm/yarn │  │
│  │  - pip/composer │  │
│  │  - bun/deno     │  │
│  └─────────────────┘  │
└────────────────────────┘
```

### Layout binaire .xbin final

```
[ ELF stub (x86_64 musl) ]
[ payload: zstd(tar(rootfs)) ]   ← v2 plain / v3 signed
[ payload: AES-256-GCM(...) ]    ← v4 encrypted
[ payload: squashfs ]            ← v5
[ metadata JSON ]                ← runtime, entrypoint, env, layers
[ SISR section ]                 ← DeltaManifest + SisrFooterExt
[ footer (84/92 bytes) ]         ← MAGIC, offsets, sha256, flags
```

---

## Refactoring du Stub Launcher — Style & Modularité

Objectif : appliquer un style de code inspiré de la rigueur Epitech/42 (structure
de fichiers modulaire, conventions de commentaires strictes, tests unitaires par
module) au `stub/src/main.rs` (2591 lignes, tests quasi-absents). Le style C
norminette ne s'applique pas directement au Rust, mais la mentalité — fichiers
courts, responsabilité unique, documentation exhaustive — oui.

### Phase 1 — Découpage en modules (WIP)

- [x] **`stub/src/seccomp.rs`** — `install_seccomp_denylist`, constants par
      architecture, `sock_filter` builder. Extrait en module testable.
      - Extraire `AUDIT_ARCH`, `SYS_*` constants dans une table par arch (enum ou
        const generics) au lieu de 60+ `#[cfg]` dupliqués.
      - Ajouter tests unitaires pour le BPF filter generation (mock prctl).

- [x] **`stub/src/namespace.rs`** — `enter_userns`, `pivot_root_into`,
      `write_proc`, `running_in_container`. Extrait en module testable.
      - Ajouter `#[cfg(test)]` pour `running_in_container` (mock des fichiers
        `/.dockerenv`, cgroup content).

- [x] **`stub/src/health_gate.rs`** — `supervised_launch`,
      `wait_for_child_status`, `wait_child_exit_code`, `ChildStatus`,
      `signal_forward`, `CHILD_PIDS`. Extrait en module testable.
      - Refactor : `ChildStatus` dans un enum partagé, `supervised_launch`
        séparé par plateforme (unix/windows déjà fait).

- [x] **`stub/src/extraction.rs`** — `extract_atomic`,
      `extract_squashfs_atomic`, `atomic_extract`, `gc_extraction_cache`,
      `slice_layers`, `cache_key_v2`. Extrait en module testable.
      - Ajouter tests pour `slice_layers` (boundary check), `gc_extraction_cache`
        (LRU eviction).

- [x] **`stub/src/exec.rs`** — `exec_app`, `resolve_entrypoint`,
      `make_resolve`, `setup_env`, `is_executable`, `check_executable`,
      `spawn_app_windows`, `find_in_bin_paths`, `supervise_services`,
      `fork_services`, `wait_for_health`, `wait_for_children`,
      `wait_for_port`, `install_signal_handler`, `signal_forward`,
      `CHILD_PIDS`, `enter_namespace_if_needed`, `detect_django_settings`,
      `detect_web_port`. Extrait en module testable (~940 lignes).

- [x] **`stub/src/crypto.rs`** — `verify_ed25519`, `decrypt_aes_gcm`,
      `chunked_decrypt_aes_gcm`, `hkdf_derive_key`, `verify_sha256`,
      `load_trusted_keys`, `trusted_keys_dir`, `cache_key_v2`,
      `slice_layers`, `hex_decode`. Extrait en module testable (~280 lignes).

- [ ] **`stub/src/update.rs`** — `maybe_apply_sisr_update`,
      `remote_update`, `HttpChunkFetcher`, `resolve_update_url`,
      `normalize_base_url`. Partiellement extrait vers `update_url.rs`
      (seulement `resolve_update_url` + `normalize_base_url`).
      - Tests déjà présents pour `normalize_base_url`, `resolve_update_url`.

- [ ] **`stub/src/runtime_flags.rs`** — `handle_runtime_flags`. Encore dans
      `main.rs` (ligne 739).
      - Tests pour `--xbin-version` / `--xbin-update` parsing.

- [x] **`stub/src/main.rs` (réduit)** — `main`, `run`, `self_exe`,
      `cache_dir`, `flock_exclusive`, FFI bindings, helpers (`cstr`,
      `to_ptr_vec`, `err`, `nanos`, `human_bytes`). Réduit de ~2229 à ~1084
      lignes après extraction de `crypto.rs` et `exec.rs`.

### Phase 2 — Convention de commentaires strictes (style Epitech/42-like)

Imposer un format de commentaires unifié pour tout le code Rust du projet
(`xbin-core`, `xbin-cli`, `stub`) :

#### Règles de commentaires

1. **Modules** (`//!`) — header de module obligatoire sur chaque fichier,
   décrivant la responsabilité et les invariants de sécurité. Template :
   ```rust
   //! One-line summary.
   //!
   //! Detailed description (2-4 lines). Mention invariants, failure modes,
   //! and how callers must use this (e.g. "must verify signature before use").
   ```

2. **Fonctions publiques** (`///` ou `/** */`) — rustdoc obligatoire, format
   inspiré des headers Epitech/42 (bloc structuré, tags `@param`/`@return`
   adaptés en markdown) :

   ```rust
   /**
    * verify_sha256 - Verify a SHA-256 digest in constant time.
    * @data: The input bytes whose hash was computed.
    * @expected: The expected 32-byte digest from the footer.
    * @return: Ok(()) if the hashes match, Err otherwise.
    *
    * Uses an XOR-fold accumulation instead of early-exit comparison to prevent
    * timing side-channels on the integrity digest.
    */
   pub fn verify_sha256(data: &[u8], expected: &[u8; 32]) -> io::Result<()> { ... }
   ```

   Ou en style `///` équivalent (rustdoc natif) :
   ```rust
   /// One-line summary (<= 72 chars).
   ///
   /// Detailed description (optional, 2-6 lines). Mention params, retours,
   /// paniques (ou "No panics"), sécurité (unsafe boundary).
   ///
   /// # Arguments
   /// * `param` — description (inline si <3 params, block si >3)
   /// # Returns
   /// Description du retour.
   /// # Errors
   /// Quand ça échoue et pourquoi.
   /// # Safety (unsafe only)
   /// Justification du safety invariant.
   ```

3. **`unsafe`** — `// SAFETY:` obligatoire avant chaque bloc `unsafe`, format :
   ```rust
   // SAFETY: <what invariant holds, why the call is safe, what the caller
   // must guarantee>. Line references to the kernel man page or spec.
   unsafe { ... }
   ```

4. **TODO/FIXME** — format obligatoire :
   ```rust
   // TODO(#123): description courte — impact + priorite
   // FIXME(#123): description — workaround actuel
   ```

5. **Commentaires inline** (`//`) — expliquent le *pourquoi*, jamais le *quoi*.
   Limiter à 100 chars. Pas de commentaires d'expiration (déjà fait).

#### CI enforcement

- [x] **`cargo doc --no-deps`** dans le CI — ajouté au job `rust` dans `.github/workflows/ci.yml`.
- [x] **`grep` vérifiant `// SAFETY:`** avant chaque `unsafe` bloc — script CI ajouté
       dans le job `rust` (vérifie tous les `.rs` de `stub/src/`).
- [x] **`grep` vérifiant `//!` header** sur chaque `.rs` — script CI ajouté dans le
       job `rust` (vérifie `stub/src/` et `xbin-core/src/` ; `xbin-cli/src/` hors
       périmètre pour l'instant car de nombreux fichiers n'ont pas encore de header).
- [x] **Clippy `missing_errors_doc` / `missing_panics_doc`** — activé en `deny` dans
       `stub/Cargo.toml` (0 violation). Reste en `allow` dans `xbin-core` et
       `xbin-cli` avec TODO: 63 fonctions dans `xbin-core` manquent la section
       `# Errors` avant bascule en `deny`.

### Phase 3 — Tests unitaires par module

Après découpage (Phase 1), chaque module doit avoir des tests couvrant au moins
50 % des lignes et 100 % des fonctions publiques :

- `seccomp.rs`: test du BPF filter (mock), constants d'arch.
- `namespace.rs`: `running_in_container` mock, `write_proc` permissions.
- `health_gate.rs`: `ChildStatus` transitions, `wait_for_child_status` timeout.
- `extraction.rs`: `slice_layers` boundary, `gc_extraction_cache` LRU,
  `cache_key_v2` déterministe.
- `exec.rs`: `make_resolve` pivot/non-pivot, `setup_env` substitution,
  `is_executable` PATH search.
- `crypto.rs`: `verify_sha256` constant-time, `chunked_decrypt_aes_gcm`
  round-trip + failure cases, `verify_ed25519` multi-key accept.

## Items de Code Review — Security & Quality Improvements

Points identifiés lors de l'audit de code (session d'exploration). Priorité par
risque, pas par planning. Tous dans des fichiers existants.

### 🔴 Critique (security impact)

- [x] **`chunk_nonce` ignore le chunk index** — nonce identique pour tous les
      chunks. Fix: incorporer le chunk index dans le nonce
      (`[base_nonce[0..4]; chunk_index: u64 be]`), garantissant un nonce
      unique par chunk. (`xbin-core/src/encrypt.rs:117`)
- [x] **`cargo audit` absent du CI** — ajouté aux jobs rust/macos/windows.

### 🟠 Haute (robustesse / maintenabilité)

- [x] **`chunk_nonce` ignore le chunk index** — fixé: nonce unique par chunk
      (`[base_nonce[0..4]; chunk_index: u64 be]`).
- [x] **`cargo audit` absent du CI** — ajouté aux jobs rust/macos/windows.
- [x] **`read_sisr` charge le manifeste en entier en mémoire**
        (`stub/src/main.rs:357`) — un manifeste malveillant de grande taille
        (chunk_count = `u32::MAX`) peut OOM le stub avant vérif. **Fix** :
        validé par `MAX_MANIFEST_SIZE = 4 MiB` dans `xbin-core/src/sisr_header.rs:20`
        et check en lecture (`sisr_header.rs:126`). Rejet propre avant parse.
- [x] **`build_reuse_index`** — `read_sisr(exe).ok().flatten()` returns an
       empty index on read/corruption error. This is **by design**: correctness
       never depends on the reuse index (only reuse efficiency does). A corrupted
       SISR manifest must not prevent the binary from running. Documented in
       `engine.rs:223-225`. No change needed.

- [x] **Stub `main.rs` = 2591 lignes, tests unitaires quasi-absents** — la
       plupart de la logique (namespace, seccomp, pivot_root, extraction) n'est
       pas testée. **Fix** : extrait en modules testables
       (`seccomp.rs`, `namespace.rs`, `extraction.rs`, `health_gate.rs`,
       `update_url.rs`, `win.rs`). `main.rs` réduit à ~2229 lignes.

- [x] **Magic numbers seccomp BPF** (`stub/src/seccomp.rs`) — les numéros de
       syscalls étaient hardcodés par architecture. **Fix** : remplacés par les
       constants `libc::SYS_*` (libc expose le bon numéro par target_arch).
       Seuls `AUDIT_ARCH` et les constantes BPF standard restent en dur
       (aucune crate Rust ne fournit `AUDIT_ARCH`).

### 🟡 Moyenne (propreté code)

- [x] **`parse_target` tests** — unit tests cover short forms (`aarch64`,
       `x86_64`), full Rust triples (`aarch64-apple-darwin`,
       `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-gnu`,
       `x86_64-pc-windows-gnu`), and OS shorthands (`win-x64`, `win-arm64`,
       `linux-x64`, `linux-arm64`). All formats documented in `parse_target`
       are tested. (`xbin-cli/src/commands/build.rs:2248-2286`)

- [x] **`find_stub` can embed an obsolete installed stub** — removed the
       `/usr/local/bin/xbin-stub` fallback entirely. Current search order:
       1) `XBIN_STUB_PATH`, 2) `CARGO_TARGET_DIR/<arch>/release/xbin-stub`,
       3) `/tmp/xbin-stub-target/<arch>/release/xbin-stub`, 4) `stub/target/<arch>/release/xbin-stub`,
       5) `which::which("xbin-stub")` with warning. Fresh builds always preferred.

- [x] **`scan.rs:227`** — `meta.get("isolation").unwrap_or(0)` already matches
       the stub's actual default behavior (u8::default() = 0). `scan` reports
       what the binary WILL DO, not a fabricated fail-closed value. Correct as-is.

- [ ] **Stub Windows** (`spawn_app_windows`, lignes 1540+) — code Windows non
      testé, paths non couverts. **Fix** : tests Windows CI + couverture.

---

## Recherche HN — Produits similaires et retours utilisateurs

### Produits comparables (extraits de discussions HN)

| Produit | Points forts (selon HN) | Points faibles (selon HN) |
|---------|------------------------|--------------------------|
| **Burrito (Elixir)** | Cross-compile depuis Linux, payload extrait 1ère fois | Erlang/BEAM runtime overhead |
| **vercel/pkg** | Facile, cross-compile ARM | Déprécié, échoue avec native addons |
| **nexe** | Anciennement populaire | Moins maintenu |
| **bun** | Speed, bundler intégré | ~90MB binaire hello world |
| **deno compile** | Single executable natif | ~70MB (V8), expérimental |
| **AppImage** | Portable Linux, squashfs | Nécessite FUSE ou extraction |
| **PyInstaller** | Python tool populaire | Slow startup, cross-platform limité |
| **Go/Rust natif** | Single binary, fast | Nécessite le langage cible |
| **Docker** | Isolation, écosystème | Overhead daemon, complexité |
| **.NET single-file** | Mature | Extraction native libs runtime |

### Leçons HN (patterns de feedback)

1. **Binary size** : People complain a LOT — bun (90MB), deno (70MB), pkg (50MB+).
   → x.bin à **44MB** est compétitif (mentionner dès le 1er paragraphe).

2. **Startup time** : critique — PyInstaller "slow startup".
   → x.bin : **95ms warm** (cache) vs **2s cold** (extraction). USP.

3. **Cross-compilation** : demandé constamment.
   → x.bin : multi-arch stub, mais **pas de cross-compile CLI → binary**.

4. **Self-update** : souvent mentionné comme manquant.
   → x.bin a **SISR delta updates** (unique — no competitor does this).

5. **Native modules** : pkg/nexe échouent.
   → x.bin : embed via `ldd_deps`, **besoin de plus de tests**.

6. **"Docker vs single binary"** : débat récurrent.
   → x.bin : **seccomp + landlock + pivot_root** = isolation sans daemon.

7. **Reproducible builds** : apprécié des devs sérieux.
   → x.bin : tarball déterministe, mais pas de lockfile verification.

### Recommandations pour Show HN

**Title** :
- *"x.bin: 44MB single ELF, 95ms warm start, 60% smaller than Docker"*
- Alternative: *"Show HN: Pack any app into a single self-extracting binary — 44MB vs Docker's 76MB"*

**Body structure** (HN-friendly) :
1. One sentence : what + killer number (44MB/95ms)
2. Problem : Docker too heavy, pkg/deno/bin too bloated
3. Benchmarks table : x.bin vs Docker/pkg/bun
4. Key differentiators : SISR delta updates, security (seccomp+landlock), warm start
5. Cross-platform status
6. GitHub link + quickstart

**Réponses anticipées** (prépare-toi) :
- "How does this compare to Burrito/docker/AppImage?"
- "Can it cross-compile from Linux to macOS/Windows?"
- "What about native modules?"
- "Is the extraction secure (tamper-proof)?"
- "How do delta updates work exactly?"

## Plan Adoption (Open Source → Hacker News → Viral)

### Phase 1 — Pre-launch (2-3 semaines)
1. **`cargo publish`** pour xbin-core, xbin-cli, xbin-stub (crates.io)
2. **README "killer"** : 1 liner + GIF demo + comparatif table
3. **GitHub Actions** : publish automatique + release binaries cross-platform
4. **`brew tap`** + `scoop` bucket (one-liner installs = adoption)

### Phase 2 — Hacker News / Reddit
1. **"Show HN"** le jour ouvrable matin (8-10h PST) : title punchy
   - *"x.bin: pack any app into a single binary — 44MB vs Docker's 76MB"*
2. **Commentaire de suivi** : répondre aux questions techniques
3. **r/rust**, **r/programming** : cross-post 1 jour après HN

### Phase 3 — Post-HN (2 semaines critiques)
1. **GitHub Stars → 100+** = trending Rust repos
2. **Fix issues en 24h** — montre activité
3. **Content marketing** : blog post "How we built constant-time crypto in 500 lines"
4. **YouTube 60s demo** : "xbin build myapp — one binary to rule them all"

### Levier clé
Ton **benchmark (44MB/95ms warm)** est le USP. Hacker News adore les benchmarks
objectifs. Mets-le en évidence dès le premier paragraphe du README.

## Items de Code Review — Security & Quality Improvements

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

## Issues Found — Code Review

### Build break found + fixed during re-verification
- `stub/src/seccomp.rs` was missing `use std::io;` — the extracted module's
  `install_seccomp_denylist()` returns `io::Result<()>` with no import in scope.
  **Fixed** by adding `use std::io;` at module top.
- **Git submodule orphan**: `.opencode/skills/avoid-ai-writing` was registered as a
  gitlink (mode `160000`) in the git tree but had no `.gitmodules` entry. This caused
  `fatal: No url found for submodule path` during CI cleanup on Windows.
  **Fixed** by `git rm --cached` to remove the gitlink (directory was empty).

### Issues FIXED (with tests, verified)

1. ✅ **`JAVA_OPTS` now split into individual JVM arguments** — `xbin-core/src/detect.rs:385-390`
   `opts.split_whitespace()` + insert each option separately. Test: `java_opts_split_into_individual_args`.

2. ✅ **Django `DJANGO_SETTINGS_MODULE` parsing now handles quotes properly** — `stub/src/exec.rs:228-252`
   Uses `rfind('"')`/`rfind('\'')` to locate the VALUE's closing quote (the first quote after
   split on `DJANGO_SETTINGS_MODULE` is the key's closing quote). Test:
   `detect_django_settings_handles_commas_and_parens`.

3. ✅ **Dead code in Node entrypoint step 2 removed** — `xbin-core/src/detect.rs:326-329`
   Removed unreachable `entry.starts_with("bun ")` / `entry == "bun"` branch.
   Test: `bun_exact_bun_falls_through_to_file_search`.

4. ✅ **Versioned `.so` files now matched** — `stub/src/exec.rs:177-182`
   Added `file_name().contains(".so.")` check alongside `ext == "so"`.

5. ✅ **`find_dotnet_self_contained` merged `read_dir` calls** — `xbin-core/src/detect.rs:560-580`
   Single iteration instead of two separate `read_dir(publish_dir)` blocks.
   Test: `detect_dotnet_self_contained`.

6. ✅ **`--xbin-update=URL` syntax now supported** — `stub/src/update_url.rs:14-19`
   Added `=` prefix parsing. Test: `resolve_update_url_with_equals_syntax`.

7. ✅ **`detect_web_port` Node.js now handles `--port=3000`** — `stub/src/exec.rs:288-295`
   Added `strip_prefix('=')` on the port value. Test: `detect_web_port_handles_port_equals_syntax`.

8. ✅ **`detect_bun_entry` `cmd == "bun"` fixed** — `xbin-core/src/detect.rs:533-553`
   Removed `cmd == "bun"` branch; exact `"bun"` now falls through to `find_first_file`
   which returns `["bun", "run", "index.js"]` instead of broken `["bun", "run", "bun"]`.

9. ✅ **`.env` parsing now uses `xbin_core::dotenv::load_dotenv`** — `stub/src/exec.rs:112-116`
   Replaced naive `split_once('=')` parser with `xbin_core::dotenv::load_dotenv(rootfs, None, false)`.
   Handles `export` prefix, quoted values, and inline comments correctly.

10. ✅ **`pyproject.toml` now parsed via TOML, not string matching** — `xbin-core/src/detect.rs`
    - `detect_python_module` (lines 425-455): `toml::Value` parsing for `[tool.fastapi]` /
      `entrypoint` keys instead of `content.contains()` + line scanning.
    - `detect_asgi_entrypoint` (lines 462-481): `toml::Value` parsing for `[tool.uvicorn]` /
      `[tool.gunicorn]` / `app` keys. Added `toml = "0.8"` dep to `xbin-core/Cargo.toml`.

### Issues STILL OPEN

(All code-review issues from commit `1ff0f81` are now fixed.)

### Verification status (final)
- `cargo fmt --check` ✅
- `cargo clippy -p xbin-core --all-targets -- -D warnings` ✅
- `cargo clippy -p xbin-stub --all-targets -- -D warnings` ✅
- `cargo clippy -p xbin-cli --all-targets -- -D warnings` ✅
- `cargo test --workspace` ✅ (352 passed, 0 failed, 1 ignored)

---

## Audit externe 2026-08-09 — Relevé d'issues (état vérifié)

Audit complet de la codebase (3 crates, stub, SISR, CLI), **re-vérifié ligne
par ligne contre l'arbre de travail courant**. Build + `fmt` + clippy + tests
(~352, 1 ignored) sont verts. De nombreux items ci-dessous étaient **déjà
corrigés avant l'audit** (working tree) ; ils sont donc marqués ✅ sans
attribution. Les items restants (`- [ ]`) sont les vrais correctifs à faire.

> Nota: plusieurs items marqués ✅ portent une ⚠️ pour un risque résiduel
> (manque de test, limitation inhérente au modèle).

### ✅ Corrigés (vérifiés dans l'arbre de travail)

1. - [x] **Fuite de la seed Ed25519 dans les binaires `--encrypt`** → résolu.
   La clé AES est désormais une `encryption_key` aléatoire distincte ; la seed
   de *signature* n'est plus embarquée
   (`xbin-core/src/encrypt.rs:23`, `xbin-cli/src/commands/build.rs:947-951`).
   ⚠️ **Résiduel** : la clé de chiffrement elle-même (`encryption_key_hex`)
   l'est toujours → voir item 3 plus bas.
2. - [x] **Filtre seccomp deny-all** → résolu : `jf=1` sur le dernier
   `jmp_eq` + `libc::SECCOMP_RET_KILL_PROCESS` (valeur kernel correcte)
   (`stub/src/seccomp.rs:4,63,105`).
   ⚠️ **Résiduel** : aucune couverture test — le chemin `--seccomp` n'est
   jamais exercé par `cargo test`. Risque de régression → voir item 4.
3. - [x] **Bombe zstd+tar** → résolu : `MAX_DECOMPRESSED_BYTES`/`MAX_FILES`
   avec itération manuelle des entrées (`stub/src/extraction.rs:14-17,30-55`).
   ⚠️ **Résiduel** : la protection n'existe PAS sur le format squashfs, qui est
   le **format par défaut** (v5) → voir item 5.
4. - [x] **`atomic_extract` avait masqué les erreurs** → résolu : un échec de
   `rename` n'est accepté que si le `.ready` marker existe déjà
   (`stub/src/extraction.rs:91-108`).
5. - [x] **`xz 0.4.6-rc.0`** → remplacé par `xz2 = "0.1"` (stable).
6. - [x] **Double RAM au cold start** → résolu : le hash s'appuie sur
   `verify_sha256_parts` (streaming, sans `payload.clone()`)
   (`stub/src/crypto.rs:148-168`, `stub/src/main.rs:254`).
7. - [x] **`ureq` sans timeout** → résolu : `timeout_connect(10s)` +
   `timeout_recv_body(30s)` (`stub/src/main.rs:868-880`).
8. - [x] **`rand::thread_rng()` pour matériel crypto** → remplacé par
   `rand::rngs::OsRng` (`xbin-core/src/encrypt.rs:6,40,47`).
9. - [x] **URL morte man pages** → `github.com/Tednoob17/x.bin/issues`
   (`xbin-cli/src/main.rs:269`).
10. - [x] **`chrono_now` réimplémenté à la main** → `time::OffsetDateTime` +
    `Iso8601::DEFAULT` ; `unix_days_to_date()` supprimé
    (`xbin-core/src/assembly.rs:322-332`).
11. - [x] **Embed-interpréteur par défaut** → les runtimes interprétés
    (python/node/php/ruby/deno) sont embarqués par défaut, plus besoin de
    `--embed-interpreter` pour eux (`xbin-cli/src/commands/build.rs:1168-1176`).
    ⚠️ **Résiduel** : Go/Binary/Java/Wasm n'ont pas de runtime hôte à embarquer
    (binaire natif) → la promesse « run without host runtime » tient pour tout
    sauf les binaires compilés, mais c'est correct.

### ❌ Restant — vrais correctifs à faire

3. - [ ] **Clé de chiffrement embarquée → obfuscation, pas confidentialité**
   (`stub/src/main.rs:141-143 CryptoMeta`, `xbin-cli/src/commands/build.rs:990,1003`).
   La seed de signature n'est plus embarquée (✅ #1), mais
   `encryption_key_hex` l'est, dans le fichier même. Quiconque possède le
   `.xbin` peut déchiffrer → `--encrypt` ne protège qu' contre l'inspecteur
   curieux, pas contre un attaquant qui tient le fichier.
   **Fix** : modèle de menace explicite dans la doc, ou passphrase/runtime-key
   non stockée dans le binaire.
4. - [ ] **Test d'intégration pour `--seccomp`** (le #2 est corrigé mais non
   testé → risque de régression). Un test qui build une app `--seccomp` et
   vérifie qu'elle démarre et sert (sinon le filtre deny-all reviendra en
   silence).
5. - [ ] **Bombe squashfs non protégée — et c'est le format par défaut**
   (`stub/src/squashfs_extract.rs:23-37`). Le #3 ne couvre que le zstd+tar.
   `extract_squashfs_blob` fait `reader.read_to_end(&mut buf)` par fichier
   sans limite de taille totale ni de nombre de fichiers → disk-fill.
   **Fix** : bornes similaires à `extract_atomic` (taille totale + compteur de
   fichiers) autour de la boucle `backhand` ; ou alimenter `payload_usize` du
   footer dans une vérification de decompression.
6. - [ ] **Traversal de chemins dans l'extraction squashfs**
   (`stub/src/squashfs_extract.rs:24-25`). `dest.join(node.fullpath.strip_prefix("/"))`
   ne rejette pas les segments `..`. Le chemin `tar` est protégé par le crate
   `tar`, mais `backhand` (v0.25) n'est pas explicitement contrôlé ici → un
   squashfs malicieux pourrait écrire en dehors de `rootfs`.
   **Fix** : normaliser `rel` et `dest.join(rel)` puis vérifier que le résultat
   reste sous `dest` (comme le fait `tar::unpack_in`), ou asserter
   `!rel.matches("..")`.
7. - [ ] **Feature creep CLI** : `--wasm`, `--cross-compile`, `--health-port`
   sont « metadata only, not functional yet » (README 121-127) mais exposés
   comme fonctionnels dans `--help`. **Fix** : `#[arg(long, hide = true)]` jusqu'à
   implémentation, ou lever la couverture.

### Optionnel — nettoyage / dette

- [ ] `reqwest` (~300 crates transitives, runtime `blocking`) pour de simples
  GET/PUT dans `remote_cache.rs`/`upgrade.rs`/`publish.rs`/`build.rs`. Configuré
  avec `rustls-tls` (AGENTS.md §112 : « no OpenSSL dependency » — correct). Le
  stub utilise déjà `ureq`. Unifier sur `ureq` → retire `tokio`/`hyper`,
  build plus léger. Non-bloquant.
- [ ] **Documenter le modèle de confiance signé** : un `.xbin` signé ne s'exécute
  pas sans la pubkey installée via `xbin trust` chez l'utilisateur final. Qui
  installe les `trusted-keys` ? (CI ? distributeur ? utilisateur ?)
    Documenter le modèle de confiance (qui installe les trusted-keys ?).
