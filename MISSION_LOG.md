# MISSION_LOG.md — Suivi des missions SISR (Prompt 1→10)

Fichier unique consignant toutes les modifications apportées au projet x.bin
dans le cadre de l'initiative SISR (Self-Incremental Sovereign
Reconstruction). Tenue à jour à chaque mission.

> **Session** : missions 1–10 terminées (03/08/2026).
> Conventions : docs mdbook en anglais (`docs/src/`), code zéro `unsafe` dans
> `xbin-core`, pas de dossier `crates/` (tout va dans `xbin-core/src/`),
> vérification imposée (fmt → clippy par crate → tests workspace → mdbook).

---

## Mission 10 — Migration v1 → v2 + rétrocompatibilité (Prompt 10/10) — TERMINÉE

Périmètre : migration, rétrocompatibilité, gouvernance de dépréciation.
Aucune nouvelle fonctionnalité majeure.

### Invariant garanti (§5)

Un binaire v1 (sans SISR) est lu, extrait et exécuté par le runtime v2 sans
modification ni avertissement superflu — c'était déjà vrai côté runtime
(`read_sisr` → `Ok(None)`, extraction standard) ; un test E2E le verrouille.

### Code — `xbin-core/src/legacy.rs` (nouveau)

- `upgrade_binary(input, output, config)` : promeut un `.xbin` classique au
  format v2 en insérant `[manifest][SisrFooterExt]` entre metadata et footer.
- Segments stub/payload/metadata copiés **byte-for-byte** ; le payload est
  chunké tel que stocké (jamais décompressé/recompressé) ⇒ somme
  `SHA-256(payload ‖ meta)` et checksums internes (SquashFS) préservées (§10).
- `payload_offset`/`meta_offset`/`footer_size` inchangés ⇒ un runtime v1
  lit toujours le fichier promu.
- Refuse : fichier déjà SISR, fichier signé (format ≥ 3), non-`.xbin`.
- Écrit `<output>.xbin.manifest` (comme `assemble_xbin_with_sisr`).
- 4 tests unitaires : segments préservés + `FLAG_SISR` + round-trip `read_sisr`
  + hash inchangé ; refus SISR/signé/non-xbin.

### CLI — `xbin upgrade-binary`

- `xbin-cli/src/commands/upgrade_binary.rs` + sous-commande dans `main.rs`.
- Args : `<input> <output>`, `--chunk-size` (défaut 64 KiB), `--key` (signe le
  manifeste SISR), `--force`, `--quiet`, `--json`.
- Test d'intégration `cli_tests.rs` : conversion réelle, `FLAG_SISR` posé,
  hash d'intégrité identique, payload identique, manifest écrit.

### Tests cross-version — `stub/tests/upgrade_migration.rs`

- `legacy_binary_runs_on_v2_runtime` : binaire v1 (assemblé sans SISR) exécuté
  par le vrai stub ⇒ sortie OK, zéro warning `[xbin]` (§5, §11).
- `upgraded_binary_gains_auto_update` : après upgrade, le binaire applique un
  vrai delta v2 via le canal mocké (serveur HTTP) et exécute la nouvelle
  payload (§8).
- `upgraded_binary_preserves_payload_bytes` : segments copiés byte-for-byte +
  hash d'intégrité identique (§10).
- `mock_server` rendu `pub` pour réutilisation des helpers E2E.

### Docs

- `docs/src/migration/v1-to-v2.md` : arbre de décision du loader, ce que change
  l'upgrade, options A (rebuild) / B (`upgrade-binary`), gouvernance de
  dépréciation, vérification. Entrée SUMMARY `# Migration`.
- `docs/src/CHANGELOG.md` (créé) : publication majeure 1.0.0 SISR. Entrée
  SUMMARY `# Project`.

### Vérif

fmt OK · clippy 3 crates OK · **288 tests** (214 core + 24 cli + 14 cli_tests
+ 10 stub unit + 10 e2e_sisr + 13 upgrade_migration + 3 health_rollback) ·
mdbook OK. Commit non signé (pas de clé GPG sur cette machine).

---

## Mission 9 — Tests : E2E + fuzzing + contraintes réseau (Prompt 9/10) — TERMINÉE

Périmètre : uniquement des tests et de la documentation. Zéro modification du
code de production.

### E2E — serveur HTTP mocké (std-only)

- `stub/tests/e2e_sisr_main.rs` : entrée `#[path = "e2e_sisr/mod.rs"]`.
- `stub/tests/e2e_sisr/mod.rs` : helpers partagés — `TestEnv`, `env(v1, v2)`
  (vrai binaire `CARGO_BIN_EXE_xbin-stub`, trusted keys, update stagé signé),
  `stage_update`, `run_update`, `run_app`, `parse_stats`.
- `stub/tests/e2e_sisr/mock_server.rs` : `MockHttpServer` 100 % std (TcpListener
  non-blocking, thread par connexion, routes HashMap, 404 par défaut). CI sans
  root ni réseau : cache isolé (`XDG_CACHE_HOME`/`XDG_DATA_HOME`),
  `XBIN_TRUSTED_DIR`, `XBIN_HEALTH_TIMEOUT_MS`.
- `update_basic.rs` : 4 tests — swap v1→v2 + stats, téléchargement borné aux
  blocs modifiés (≤ changé + 2 %), le binaire mis à jour s'exécute, chemin de
  staging local de la mission 6 toujours opérationnel.
- `update_failures.rs` : 6 tests de refus — signature non fiée, manifeste
  corrompu, chunk manquant (404), chunk tronqué, bytes corrompus (SHA-256),
  racine Merkle divergente. Chaque refus laisse le binaire intact sans `.bak`.
- Résultat : 10 tests, ~12 s (< 30 s CI).

### Fuzzing contraintes réseau (engine)

- `xbin-core/src/sisr/network_test.rs` : `FaultInjectingFetcher` (wrapper du
  trait `ChunkFetcher`) + 6 tests — latence (résultat inchangé), coupure de
  connexion (`ConnectionReset`, binaire intact), corruption (SHA-256), paquets
  tronqués, débit lent (reconstruction correcte), comptabilisation des octets.

### Fuzzing property-based (proptest, stable)

- `proptest = "1"` en dev-deps de `xbin-core`.
- `manifest.rs`, `sisr_header.rs`, `format.rs` : modules `proptests` — jamais de
  panic sur bytes arbitraires, round-trips `pack`/`parse` et
  `serialize`/`parse` sans perte, buffers tronqués toujours rejetés.
- `fuzz/fuzz_targets/sisr_manifest.rs` + `fuzz/Cargo.toml` : cible libFuzzer
  (nightly uniquement, hors workspace membres et hors CI) couvrant
  `DeltaManifest::parse`, `RemoteManifest::from_bytes`, `Footer::read_from`,
  `read_sisr`.

### Docs

- `docs/src/contributing/testing.md` (+ entrée SUMMARY) : les 4 couches de
  tests, commandes, notes CI (< 30 s, sans root, proptest = surface fuzz stable).

### Vérif

fmt OK · clippy 3 crates OK · **270 tests** (210 core + 24 cli + 13 cli_tests +
10 stub unit + 10 e2e_sisr + 3 health_rollback) · mdbook OK. Commit non signé
(pas de clé GPG sur cette machine).

---

## Mission 8 — Rollback automatique + Vérification de santé post-reconstruction (Prompt 8/10) — TERMINÉE

### Objectif
Sécuriser l'auto-update contre les **mauvaises versions** : un update SISR
atomique peut être parfaitement appliqué puis **crasher au démarrage**. Mission
8 ajoute (1) un snapshot `.xbin.bak` du binaire courant avant le swap, (2) un
**Health Check Gate** au premier run de la nouvelle version (supervision
fork+waitpid pendant une fenêtre de démarrage), (3) un **rollback automatique
atomique** si la nouvelle version échoue, (4) une **quarantaine de version**
(anti-boucle) qui refuse de ré-installer une version qui a déjà échoué.
Le « Cache Distant » du titre n'était pas détaillé dans le prompt — non traité.

### Décisions de conception (ENREGISTRÉES — modifiables ensuite)
1. **Snapshot co-localisé** : `.xbin.bak` à côté du binaire (même filesystem) ⇒
   restauration = un seul `rename(2)` atomique. Il n'existe que pendant la
   validation (créé avant le swap, supprimé à la confirmation ou au rollback).
   Permissions préservées (bit exec).
2. **Health Gate = superviseur fork+waitpid**, pas un exec direct : l'exec seul
   ne peut pas détecter un crash. Parent fork → enfant exec l'app ; parent
   `waitpid(WNOHANG)` 50 ms jusqu'à `XBIN_HEALTH_TIMEOUT_MS`. Exited 0 ou
   encore vivant ⇒ confirmé sain ; crash ou exit non-zéro ⇒ échec enregistré.
3. **Compteur d'échecs non remis à zéro** : `begin()` (ré-install) préserve
   `attempts` — ré-installer la même version cassée s'additionne, pas de
   resynchronisation de l'horloge d'échecs.
4. **Quarantaine** quand `attempts >= XBIN_HEALTH_MAX_ATTEMPTS` (défaut 3,
   env override, `0` = jamais). Un `begin()` ne ré-arme jamais une version
   Quarantined. Nettoyage manuel = supprimer le fichier JSON.
5. **Anti-boucle avant I/O** : `refuse_quarantined_target` ne calcule le hash
   cible (dry-run coûteux) **que si** le store contient une quarantaine —
   sinon no-op. Refus ⇒ le binaire courant n'est jamais touché.
6. **Store = JSON par version** dans `~/.cache/xbin/health/<sha256>.json`
   (`XDG_CACHE_HOME`-relatif), écritures atomiques (tmp+rename) ; version id =
   `footer.sha256_hex()` = SHA-256(payload ‖ meta).
7. **Rollback re-exec** : après restauration, retrait de `XBIN_SISR_MANIFEST`
   et `XBIN_UPDATE_URL` puis `execvp` du binaire restauré (argv d'origine) —
   le process frais relit la footer de l'ancienne version et exécute l'app.
   Exit status enfant décodé : signal ⇒ `128+sig` (conforme wait(2)).

### Code
- `xbin-core/src/sisr/health.rs` (nouveau) : `HealthState { Pending, Healthy,
  Quarantined }` (serde snake_case), `HealthCheckPolicy { timeout_ms,
  max_attempts }` (defaults 10 000 ms / 3), `HealthStatus`, `HealthStore`
  (load/begin/confirm/record_failure/has_quarantined ; `record_failure ->
  bool` quarantaine ; `max_attempts==0` jamais).
- `xbin-core/src/sisr/resilience.rs` (nouveau) : `BACKUP_SUFFIX=".bak"`,
  `backup_path_for`, `create_backup` (bytes + perms via `AtomicWriter`),
  `restore_backup` (NotFound si snapshot absent), `discard_backup`
  (idempotent).
- `xbin-core/src/sisr/engine.rs` : `SisrEngine::target_payload_sha256` =
  SHA-256(payload ‖ meta) sans écriture (utilisée pour comparer la version
  cible sans risquer d'effet de bord) ; `write_chunk` refactorisé en
  `resolve_chunk_bytes`/`fetch_verified` partagés entre le chemin d'écriture
  et le dry-run (reuse-hash-vérifiée sinon fetch-vérifiée).
- `xbin-core/src/sisr/mod.rs` : `pub mod health; pub mod resilience;`
- `stub/src/main.rs` (⚠️ crate sensible — validé) :
  - `bin_path = fs::canonicalize("/proc/self/exe")` recapturé après l'update
    et réutilisé pour le gate santé / rollback.
  - Gate santé inséré après extraction de la nouvelle version : `Pending` →
    `supervised_launch` ; `Quarantined` → `rollback_to_previous`.
  - `maybe_apply_sisr_update` / `remote_update` refactorisés :
    `refuse_quarantined_target` → `apply_with_rollback_snapshot` (backup →
    apply → sur erreur discard backup → sur succès `mark_pending_after_update`
    via `store.begin(footer.sha256_hex())`).
  - Nouvelles fonctions : `health_store_dir` (= `cache_dir()/health`),
    `health_policy` (env `XBIN_HEALTH_TIMEOUT_MS`/`XBIN_HEALTH_MAX_ATTEMPTS`),
    `refuse_quarantined_target`, `apply_with_rollback_snapshot`,
    `mark_pending_after_update`, `ChildStatus { StillRunning, Exited(i32),
    Signaled(i32) }`, `supervised_launch`, `wait_for_child_status`,
    `wait_child_exit_code`, `decode_exit_status`, `rollback_to_previous`,
    `exec_again`.
- `stub/Cargo.toml` : `[dev-dependencies]` xbin-core, tempfile, tar, zstd,
  hex, sha2, ed25519-dalek (pour l'E2E du health gate).
- `stub/tests/health_rollback.rs` (nouveau) — E2E sur le **vrai binaire stub**
  (`CARGO_BIN_EXE_xbin-stub` embarqué dans un `.xbin` construit par
  `assemble_xbin_with_sisr`, clé déterministe, cache isolé via `XDG_CACHE_HOME`,
  trusted-keys via `XBIN_TRUSTED_DIR`) :
  1. `crashing_update_is_rolled_back_and_old_version_runs` : v2 `exit 1` ⇒
     rollback atomique, disque = v1, `.bak` supprimé, v2 `Quarantined`.
  2. `healthy_update_is_confirmed_and_kept` : v2 `exit 0` ⇒ confirmé
     `Healthy`, `.bak` supprimé, disque = v2.
  3. `quarantined_version_is_refused_on_reinstall` : 1er run crashe ⇒
     quarantaine (avec `XBIN_HEALTH_MAX_ATTEMPTS=1`) ; 2e run avec le même
     manifest ⇒ « quarantined » sur stderr, exit non-zéro, binaire = v1.

### Tests
+3 E2E stub (rollback, confirmation, quarantaine anti-loop) + unités health
(load/begin/confirm/record_failure/has_quarantined, préservation compteur,
non-réarmement, seuil, 0=max jamais) + unités resilience (chemin `.bak`,
snapshot bytes+mode, restore atomique, discard idempotent, backup intouché)
+ engine `target_payload_sha256` (hash sec == footer.payload_sha256 du
rebuild ; dry-run ne modifie pas le binaire) ⇒ **xbin-core 196 tests**.

### Docs
- `docs/src/concepts/rollback-and-resilience.md` (créé) + entrée SUMMARY.
- `docs/src/architecture/runtime-launcher.md` : étape 7 « health gate » dans le
  flux, section « Post-update health gate », ligne failure table.
- `docs/src/architecture/internal-crates.md` : arbre `sisr` + health/resilience.
- `docs/src/guides/user-updates.md` : point 5 du modèle de sécurité + table
  de failure (crash ⇒ rollback, quarantaine).

### Vérif
fmt OK · clippy 3 crates `-D warnings` OK · tests workspace OK (196 core + E2E
stub) · mdbook OK.

---

## Mission 7 — CLI SISR + auto-update runtime (Prompt 7/10) — TERMINÉE

### Objectif
Exposer SISR dans le CLI `xbin` et dans le runtime d'auto-update : flags
`--enable-sisr`/`--key`/`--update-url` à `xbin build`, sous-commande runtime
cachée `--xbin-update` interceptée par le launcher **avant** transmission des
args à l'app hôte, affichage clair (progress, stats blocs réutilisés vs
téléchargés), docs `cli/xbin-build.md` + `guides/user-updates.md`.

### Décisions de conception (ENREGISTRÉES — modifiables ensuite)

⚠️ **Ces choix sont consignés ici car des modifications de conception sont
possibles plus tard. Ils ne sont PAS gravés dans le marbre.**

1. **Transport réseau du stub = HTTPS (validé par l'utilisateur)** : le stub
   était « network-free » (décision mission 6 : staging local + env
   `XBIN_SISR_MANIFEST`). Pour Prompt 7, l'utilisateur a choisi que **le stub
   télécharge lui-même** manifeste + chunks via **`ureq` + rustls**
   (dép. ajoutée à `stub/Cargo.toml`). Le chemin `XBIN_SISR_MANIFEST` local
   reste supporté (aucune régression). Contexte de sécurité : le manifeste est
   signé Ed25519 et les chunks sont content-adressés (SHA-256 imposé par le
   moteur) — la confiance est dans la signature, pas dans le transport ; on
   fait quand même HTTPS (choix ANSSI-friendly).
2. **Flags builder** : `--enable-sisr` (bool, nouveau) ; `--key <PATH>`
   (**réutilise le flag existant**) ; `--update-url <URL>` (nouveau, stocké
   dans les métadonnées → champ `update_url` du JSON meta). `chunk_target_size`
   fixé à 64 KiB (défaut SISR).
3. **Sémantique de `--key` quand `--enable-sisr` est actif** : `--key` signe le
   **manifeste SISR** (et non le binaire) — conforme au prompt (« Clé privée
   Ed25519 pour signer le manifeste »). Le format de clé est le même que la
   signature binaire (32 octets bruts, cf. `keygen.rs`/`sign.rs`). Donc avec
   `--enable-sisr`, on **n'applique pas** `sign_file` au binaire dans le même
   build (sinon `sign_file` tronquerait la section SISR : il reconstruit le
   fichier en `original[0..meta_end] + sig_block + footer`, ce qui détruirait
   `[manifest][ext]`). ⚠️ Point d'attention : signature binaire + SISR
   simultanés non supportés dans un seul `xbin build` (documenté) ; à réviser
   si un cas d'usage l'exige.
4. **Stockage de l'URL d'update** : champ `update_url` ajouté au JSON meta
   (`MetaOptions.update_url` dans `xbin-core/src/assembly.rs` +
   `Metadata.update_url` dans `stub/src/main.rs`, serde default). Résolution
   au runtime : arg positionnel de `--xbin-update` > env `XBIN_UPDATE_URL` >
   `meta.update_url`.
5. **Convention URL du serveur d'update** : `{base}/manifest` (fichier XBMR
   = `<out>.manifest`) et `{base}/chunks/<64-hex-sha256>` (contenu par hash).
   `base` = `--update-url` sans slash final. À documenter côté éditeur.
6. **Interception dans le stub** : lecture de `std::env::args_os()` au début de
   `run()`. `--xbin-update [URL]` → mode update **terminal** (applique puis
   exit) ; `--xbin-version` → affiche la version puis exit. Les deux ne sont
   **jamais transmis** à l'app hôte. Sinon, comportement actuel inchangé
   (invariant : sans `--enable-sisr`, binaire strictement identique + UX
   inchangée ; les args passent tels quels à `exec_app`).
7. **Affichage** : progress par chunk fetché (`fetched i/N`), stats finales
   réutilisés/téléchargés (chunks + octets) via `apply_update_with_stats`,
   tout sur **stderr** (stdout réservé à l'app).
8. **Sécurité clé (Prompt §10)** : masquer la clé privée dans les logs CLI
   (jamais imprimer les octets ; le chemin est OK) ; **vérifier perms 0600
   sous Unix** → warning si pas 0600 (pas d'échec dur, pour ne pas casser les
   flux existants).
9. **Pas de dossier `crates/`** : Prompt mentionne
   `crates/xbin_cli/src/commands/build.rs` et `crates/xbin_runtime/src/args.rs`
   ⇒ traduits vers la structure réelle : `xbin-cli/src/commands/build.rs` et
   runtime dans `stub/src/main.rs` (pas de crate `xbin_runtime`).

### À faire (todo)
1. `build.rs` : + `--enable-sisr`, `--update-url` ; logique SISR
   (`SisrBuildConfig` 64 KiB + `assemble_xbin_with_sisr` + sign manifeste
   `--key` + 0600 + JSON/dry-run).
2. `xbin-core`: `MetaOptions.update_url` + `build_meta_json` ; stub
   `Metadata.update_url`.
3. `stub`: interception `--xbin-update`/`--xbin-version` (args_os), résolution
   URL, `HttpChunkFetcher` (ureq), progress + stats (stderr), `--xbin-version`.
4. `stub/Cargo.toml`: + `ureq` (rustls).
5. Tests : acceptation CLI (`build --enable-sisr`, non-interférence args),
   unitaires stub (parsing args/URL).
6. Docs : `cli/xbin-build.md`, `guides/user-updates.md`, SUMMARY,
   mise à jour `runtime-launcher.md` (plus « network-free ») +
   `incremental-updates.md` (flags `--self-update`/`update` → `--enable-sisr`/
   `--xbin-update`).
7. Boucle vérif (fmt, clippy 3 crates, tests, mdbook, release) + E2E manuelle
   (HTTP local).

### Résultats (validation 03/08/2026)

- **Code** : `build.rs` (+ `--enable-sisr`, `--update-url`, `build_sisr_config`,
  `warn_if_insecure_key_permissions`, sign manifeste vs binaire),
  `xbin-core/src/assembly.rs` (`MetaOptions.update_url`), `stub/src/main.rs`
  (`handle_runtime_flags`, `resolve_update_url`, `normalize_base_url`,
  `remote_update`, `HttpChunkFetcher`, `human_bytes`, `Metadata.update_url`),
  `stub/Cargo.toml` (+ `ureq = "3"`).
- **Tests** : 24 unitaires CLI (dont 4 `build_sisr_config_*`), 13 d'acceptation
  CLI (dont 3 dry-run SISR), 10 unitaires stub (dont 5 parsing URL) —
  **226 tests workspace OK**. Les 2 tests dry-run ont été réparés en ajoutant
  un runtime détectable (`app/package.json`) au répertoire de test.
- **Docs** : `cli/xbin-build.md` (nouveau), `guides/user-updates.md` (nouveau),
  SUMMARY à jour, `runtime-launcher.md` (plus « network-free » : déclencheur
  `--xbin-update` + `HttpChunkFetcher`), `incremental-updates.md` (flags
  `--self-update`/`update` → `--enable-sisr`/`--xbin-update`).
- **Boucle vérif complète** : fmt, clippy `-D warnings` sur les 3 crates,
  `cargo test --workspace` (226 OK), `mdbook build docs/`, `cargo build
  --release` — tout passe.
- **Commit** : première grosse passe git de l'initiative SISR (missions 1–7),
  voir commit « 6393358..HEAD ».

---

## Mission 6 — Runtime : SisrEngine dans le launcher (Prompt 6/10) — TERMINÉE

### Objectif
Implanter le moteur de reconstruction incrémentale autonome dans le runtime de
démarrage : réutilisation des blocs inchangés du binaire courant, téléchargement
des blocs manquants, vérification hash, assemblage local, remplacement atomique.

### Décisions (validées par l'utilisateur)
- Modules dans `xbin-core/src/sisr/` (`engine.rs` + `swap.rs`), pas de crate
  `xbin_runtime` (convention « pas de `crates/` » ; le stub dépend déjà de
  `xbin-core`).
- Câblage dans `stub/src/main.rs` via variable d'environnement
  `XBIN_SISR_MANIFEST` (pas de nouvelle commande CLI — Prompt 7).

### Code
- `xbin-core/src/sisr/mod.rs` (nouveau) : `pub mod engine; pub mod swap;`
- `xbin-core/src/sisr/engine.rs` (nouveau) : trait `ChunkFetcher`,
  `DirectoryChunkFetcher` (racine `<root>/<hex-hash>`), `SisrUpdateStats`,
  `SisrEngine::{apply_update, apply_update_with_stats}`. Index de réutilisation
  lu du manifeste embarqué du binaire courant (`read_sisr`, échec ⇒ index vide
  = fetch complet, correct) ; chaque chunk écrit (réutilisé **ou** fetché) est
  vérifié `SHA-256 == hash` ; footer reconstruit (FLAG_SISR, sig_offset zéro,
  hash cumulatif payload+meta) ; **mode du fichier source copié sur le .tmp
  avant rename** (sinon le binaire remplacé perd le bit exec).
- `xbin-core/src/sisr/swap.rs` (nouveau) : `AtomicWriter` RAII (`.tmp-<pid>`,
  flush + `sync_all` + `rename` ; Drop supprime si non commité) +
  `atomic_replace`.
- `xbin-core/src/lib.rs` : `pub mod sisr;`
- `xbin-core/src/sisr_stage.rs` : + `RemoteManifest::verify_any(&[VerifyingKey])`
  (accepte l'update si **une** clé de confiance vérifie la signature).
- `stub/src/main.rs` :
  - `read_from(path)` (extrait de `read_self`) ; flux : `read_from(/proc/self/exe)`
    → si `XBIN_SISR_MANIFEST` : parse `RemoteManifest` → vérif signature
    trusted-keys (`load_trusted_keys` factorisé de `verify_ed25519`) → vérif
    Merkle → `SisrEngine::apply_update` (chunks dans `<manifest-dir>/chunks/`)
    → **re-open du chemin canonique retourné** (pas `/proc/self/exe`, qui peut
    résoudre vers l'inode pré-update) → extraction/exec normal.
  - `verify_ed25519` refactorisé sur `load_trusted_keys()` (comportement
    identique).

### E2E manuelle (binaire stub réel, /tmp)
Harness `/tmp/opencode/xbin-e2e` (hors repo, dépendance path à xbin-core).
Rootfs : hello statique + 512 KiB aléatoire incompressible partagé.
- Baseline v1 → « hello v1 » ; update v2 → « hello v2 » (2/13 chunks fetchés,
  **11 réutilisés**) ; re-exécution après update → v2 persiste ; perms 755
  conservées.
- Manifest corrompu (magic) → refusé, binaire intact. Signature falsifiée →
  « update manifest signature verification failed », binaire intact. Chunks
  manquants → refusé, binaire intact et v1 s'exécute toujours.

### Bugs trouvés par l'E2E (corrigés)
1. `/proc/self/exe` peut continuer de pointer l'inode pré-update après le
   rename (procfs pinne l'image en cours) ⇒ re-open du chemin canonique
   retourné par `apply_update` (testé en e2e : « hello v2 » après update).
2. `File::create` ⇒ 0644 ⇒ le rename retirait le bit exec du binaire remplacé
   (exit 126 au lancement suivant) ⇒ mode source copié avant commit + test
   `replaced_binary_keeps_the_executable_bit`.

### Tests
+13 (12 engine/swap + 1 `verify_any`) puis +1 exec-bit ⇒ **213 au total**
(179 core + 20 cli + 10 cli_tests + 4 stub).

### Docs
- `docs/src/architecture/runtime-launcher.md` (créé) + entrée SUMMARY
- `docs/src/architecture/internal-crates.md` : arbre + section `sisr`

### Vérif
fmt OK · clippy 3 crates `-D warnings` OK · 213 tests OK · mdbook OK ·
release OK.

---

## Mission 5 — Builder : pipeline d'assemblage SISR (Prompt 5/10) — TERMINÉE

### Code
- `xbin-core/src/sisr_stage.rs` (nouveau) :
  - `SisrBuildConfig { enabled, chunk_target_size, signing_key }`
  - `build_artifacts(payload, config)` : FastCDC → `DeltaManifest` → racine
    Merkle → signature Ed25519 sur `merkle_root ‖ manifest_bytes`
  - `RemoteManifest` : format fichier `.xbin.manifest` (magic `XBMR`, 104 o
    d'entête : magic+version+réservé+merkle+signature, puis `DeltaManifest`),
    `verify_signature()` / `verify_merkle()`
  - Clé `SigningKey` ed25519-dalek → zeroize au drop (feature std)
- `xbin-core/src/assembly.rs` :
  - `assemble_xbin_with_sisr(...)` : layout
    `[stub][payload][metadata][DeltaManifest][SisrFooterExt][footer]`,
    `FLAG_SISR`, écriture du manifeste distant `<name>.xbin.manifest`
  - désactivé ⇒ octets strictement identiques à `assemble_xbin` (testé)
  - `assemble_xbin` délègue à la variante SISR (`disabled()`)
- `xbin-core/Cargo.toml` : + `ed25519-dalek = "2"` (cache cargo, réseau limité)
- `xbin-core/src/lib.rs` : `pub mod sisr_stage;`

### Perf (mesurée, i5-7300U @1.6 GHz sans SHA-NI)
- 100 MiB, cibles 64 KiB : 5,3 s (19 MiB/s), 812 chunks, 29 KiB de manifeste.
- Sondé via test ignoré `perf_sisr_on_100_mib` (release).

### Tests
+12 : roundtrip sign/verify, Merkle content-binding, tiling des chunks,
rejet tampering, byte-identique désactivé, intégration `read_sisr` + manifest
distant.

### Docs
- `docs/src/architecture/builder-pipeline.md` (créé) + SUMMARY
- `docs/src/spec/xbin-format-v2.md` : signature sur `merkle_root ‖ manifest`,
  section Remote manifest
- `docs/src/architecture/internal-crates.md` : arbre + section `sisr_stage`

### Vérif
fmt OK · clippy 3 crates `-D warnings` OK · 199 tests OK · mdbook OK ·
release OK.

---

## Mission 4 — Spécification + header SISR + manifeste de deltas (Prompt 4/10) — TERMINÉE

### Code
- `xbin-core/src/sisr_header.rs` : `SisrFooterExt` (110 o : `sisr_version:u16`,
  `chunk_table_offset:u64`, `chunk_table_len:u32`, `merkle_root:[u8;32]`,
  `signature:[u8;64]`) — sans `repr(packed)` (zéro `unsafe`), sérialisation
  little-endian explicite `pack()`/`parse()`/`read_from()` + `read_sisr()`
  (bounds-checked, overflow checked).
- `xbin-core/src/manifest.rs` : `DeltaManifest` binaire compact (magic `XBMD`,
  version u8, `chunk_count:u32`, `payload_len:u64`, table
  `ChunkEntry{hash:[u8;32], length:u32}` ; `HEADER_SIZE=20`, `ENTRY_SIZE=36` ;
  parse exact `20 + 36·n`, math checked avant allocation).
- `xbin-core/src/format.rs` : `FLAG_SISR = 0x04`, `SISR_FOOTER_EXT_SIZE = 110`,
  `Footer::footer_size()` (84 v2 / 92 v3+), `Footer::has_sisr()`.
- `xbin-core/src/lib.rs` : `pub mod manifest; pub mod sisr_header;`

### Tests
Roundtrip bit-à-bit, buffers tronqués, mauvais magic/version, count hostile
sans allocation, offsets hors bornes/overflow, absence SISR,
`size_is_110_bytes`, `header_overhead_stays_under_4kib`.

### Docs
- `docs/src/spec/xbin-format-v2.md` (créé) : layout physique, table d'octets,
  rétrocompat, sécurité, perf + entrée SUMMARY
- liens croisés `internal-crates.md` + `delta-manifest-format.md`

### Vérif
fmt OK · clippy 3 crates OK · **187 tests** (153 core + 20 cli + 10 stub +
4 launcher) · mdbook OK.

---

## Missions 1–3 — Fondations SISR — TERMINÉES

### Mission 1 — Todo + documentation
- Todo `ROADMAP.md` (initiative SISR ajoutée).

### Mission 2 — Abstractions Rust
- `xbin-core/src/chunker.rs` : trait `Chunker`, `FastCDC{min,avg,max}`, table
  gear déterministe.
- `xbin-core/src/cas.rs` : trait `ObjectStore`, `MemoryStore`, `DiskObjectStore`
  (vérification SHA-256 en lecture/écriture).
- `xbin-core/src/assembler.rs` : trait `BinaryAssembler`, `XbinStitcher`.
- Test assembler corrigé (attendu = `stub + blocks + meta + tail 92 o`).

### Mission 3 — Perf Chunker
- Débit scan FastCDC ~390 MB/s (> 200 objectif) ; SHA-256 ~50 MB/s (CPU sans
  SHA-NI, 1,6 GHz) ; documenté.
- `sha2` feature `compress` (SHA-NI dispatché runtime) activée.

### Vérif
fmt OK · clippy OK · tests workspace OK · mdbook OK.

---

## Rappels contraintes (sans cesse actives)

- `export PATH="$HOME/.cargo/bin:$PATH"` avant `cargo`/`mdbook`.
- Vérif avant clôture : `cargo fmt --check` → `cargo clippy -p <crate>
  --all-targets -- -D warnings` (par crate) → `cargo test --workspace` →
  `mdbook build docs/`.
- Zéro `unsafe` dans `xbin-core`/`xbin-cli` ; `unsafe` (stub) avec commentaires
  `SAFETY`.
- Pas de `panic!`/`unwrap` sans contexte en bibliothèque ; arithmétique
  checked ; pas de fuite mémoire.
- SISR désactivé ⇒ binaire strictement identique à l'ancien format.
- Ne pas modifier `format.rs` sans bump de version.
