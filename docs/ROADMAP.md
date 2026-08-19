# erebus — ROADMAP

## Vision

erebus est un **format d'artefact exécutable universel**, capable de transporter :

- une application classique ✅
- un agent IA (modèle + prompt + runtime) — objectif principal
- un service/microservice — objectif
- un plugin/extension — objectif

> Le format est conçu comme une unité autonome = stub + runtime + payload + metadata + signature.
> Rien n'est hérité du système hôte.

---

## Use case clé : Agent IA

Un agent IA est un binaire erebus qui contient :

```
[stub][runtime (python3)][model (gguf/bin)][prompt (toml)][tools (scripts/)][config]
```

**Ce que le format permet aujourd'hui :**
- Packager modèle + prompt + runtime dans un seul binaire portable
- Signature Ed25519 pour vérifier l'origine du modèle
- Chiffrement AES-256-GCM pour protéger les poids du modèle
- SISR pour update delta du modèle sans tout re-télécharger

**Ce qui manque (et pourquoi les couches sont nécessaires) :**
- **Hot-swap du modèle** : remplacer la couche `ModelLayer` (nouveaux poids) sans extraire le runtime, le prompt, ni les outils. Aujourd'hui le stub extrait tout le rootfs d'un coup — il faudrait extraire couche par couche et ne remplacer que la couche impactée.
- **Lazy loading** : ne charger le modèle (2-7 Go) que lorsqu'il est appelé, pas au démarrage du process. Le stub devrait pouvoir monter les couches à la demande via FUSE ou un mapping mémoire.
- **Multi-agent** : un seul binaire contenant 2-3 agents (chat, code, recherche) avec un entrypoint par agent. Le stub résout quel agent lancer selon les arguments.
- **Outils dynamiques** : les scripts/tools sont une couche séparée — on peut les mettre à jour (nouvelles APIs, nouveaux outils) sans toucher au modèle.

> Sans intégration des couches dans le stub (Phase 1), aucun de ces cas d'usage n'est possible.

---

## État actuel (août 2026)

### Couche cœur (`erebus-core`)
- ✅ CAS — `cas.rs` : `ObjectStore` trait, `MemoryStore`, `DiskObjectStore`
- ✅ Format binaire — `format.rs` : v2 (plain), v3 (signed), v4 (encrypted), v5 (squashfs)
- ✅ Metadata riche — `metadata.rs` : `ArtifactMetadata` avec `Vec<SerializableLayer>`
- ✅ Signature Ed25519 — `crypto.rs`
- ✅ Chiffrement AES-256-GCM — `encrypt.rs`
- ✅ Détection runtime — `detect.rs` : 11 runtimes, `EntrypointRegistry`

### Abstraction des couches (`layer.rs`)
- ✅ Trait `Layer` : `name()`, `kind()`, `payload_sha256()`, `compression()`, `encryption()`, `capabilities()`
- ✅ Trait `Entrypoint` : `layer()`, `execute()`, `health_check()`
- ✅ `Capability` enum : `ReadFile`, `WriteFile`, `Network`, `Exec`, `Syscall`, `Env`
- ✅ `EntrypointRegistry` : registre dynamique de runtimes
- ✅ Types concrets : `RuntimeLayer`, `ModelLayer`, `ToolLayer`, `ConfigLayer`
- ✅ `LayerKind` : `Runtime`, `Model`, `Tool`, `Config`, `Custom`

### SISR / Delta updates (`sisr/`)
- ✅ `SisrEngine` : réutilisation de chunks, vérification SHA-256, swap atomique
- ✅ `HealthStore` : quarantaine, suivi des échecs
- ✅ `AtomicWriter` : remplacement atomique de fichiers
- ✅ Tests réseau avec injection de fautes

### Assemblage
- ✅ `assemble_erebus` : builder unifié avec `AtomicWriter`, aware SISR
- ✅ Manifest distant : `.erebus.manifest`

### CLI (`erebus-cli`)
- ✅ `build`, `inspect`, `scan`, `sign`, `verify`, `keygen`, `trust`
- ✅ `doctor`, `env`, `clean`, `completion`, `man`, `upgrade`
- ⚠️ `publish` — stub/placeholder, pas de vrai push/pull

### Stub launcher (`erebus-stub`)
- ✅ Lecture footer/metadata depuis `/proc/self/exe`
- ✅ Cache SHA-256, vérification d'intégrité
- ✅ Extraction zstd+tar ou squashfs
- ✅ `execvp` entrypoint
- ✅ SISR delta update
- ❌ Ne parcourt PAS les couches dynamiquement (lit un Metadata plat)
- ❌ N'utilise PAS le trait `Entrypoint` (a sa propre logique dans `exec.rs`)
- ❌ N'applique PAS les `Capability` au runtime
- ❌ Pas de lazy loading — extrait tout le rootfs d'un coup
- ❌ Pas de hot-swap — rebuild complet pour changement d'une couche

---

## Ce qui reste à faire

### Phase 1 : Intégrer les couches dans le stub (bloquant)

**Objectif** : Le stub doit consommer le système de couches, pas le contourner.
**Bloque** : agent IA, hot-swap, lazy loading, multi-agent, capabilities.

1. **Faire lire `ArtifactMetadata.layers` par le stub** au lieu du `Metadata` plat
2. **Utiliser `EntrypointRegistry`** dans `exec.rs` pour résoudre l'entrypoint
3. **Extraction couche par couche** — extraire chaque couche individuellement, pas tout le rootfs en bloc
4. **Supprimer l'ancien `Metadata` plat** une fois la migration terminée

> Sans ça, le système de couches est du code mort et aucun use case avancé n'est possible.

### Phase 2 : Hot-swap de couches (3-4 jours)

**Objectif** : Remplacer une couche sans rebuild complet du binaire.

1. **Commande `erebus swap <binary> <layer-name> <new-layer-file>`**
   - Lit le footer du binaire existant
   - Remplace la couche cible dans le rootfs extrait
   - Recalcule l'intégrité SHA-256
   - Écrit le nouveau binaire via `AtomicWriter`
2. **SISR-aware** : si le binaire a SISR activé, génère un delta update au lieu de réécrire tout le binaire
3. **Use case agent** : `erebus swap agent.ere model ./nouveaux-poids.gguf` — remplace le modèle sans toucher au runtime, prompt, ni outils

### Phase 3 : Lazy loading (4-5 jours)

**Objectif** : Ne charger que les couches nécessaires, à la demande.

1. **Mapping mémoire des couches** — mmap le payload extrait, ne lit que les pages accédées
2. **Montage FUSE optionnel** — monter le rootfs sans extraction complète
3. **Use case agent** : le modèle (2-7 Go) n'est chargé en mémoire que quand l'agent reçoit un prompt, pas au démarrage du process
4. **CLI `erebus preload <binary> --layers model,config`** — pré-charger des couches spécifiques

### Phase 4 : Registry CAS (3-4 jours)

**Objectif** : Publier/charger des couches via un registre distant.

1. **Créer `Registry`** dans `erebus-core` au-dessus de `ObjectStore`
   - `pub fn push_layer(&self, layer: &dyn Layer) -> Result<String>` (retourne le hash)
   - `pub fn pull_layer(&self, hash: &str) -> Result<Box<dyn Layer>>`
   - `pub fn publish_artifact(&self, artifact: &ArtifactMetadata) -> Result<String>`
   - Transport : HTTP/HTTPS avec authentification token
2. **CLI `erebus registry push/pull/list`**
3. **Intégrer au build** : `erebus build --publish` pousse les couches automatiquement
4. **Use case agent** : partager le runtime python3 entre plusieurs agents (une seule couche runtime dans le registry, chaque agent référence le hash)

### Phase 5 : Security audit + enforcement (2-3 jours)

**Objectif** : Les `Capability` doivent être vérifiées, pas juste déclarées.

1. **Créer `SECURITY-AUDIT.md`** — inventaire des surfaces d'attaque
2. **Enforcer les capabilities dans le stub** :
   - `Network` → Landlock LSM ou seccomp (deny `connect`)
   - `WriteFile` → Landlock (deny write hors rootfs)
   - `Exec` → seccomp (deny `execve` sauf entrypoint)
   - `Syscall` → filter syscalls whitelist/blacklist
3. **Tests de sandboxing** — vérifier que les capabilities restreintes bloquent réellement
4. **Use case agent** : un agent IA ne doit PAS pouvoir écrire sur le filesystem hôte ni faire de réseau sauf vers ses APIs autorisées

### Phase 6 : Templates + multi-agent (2-3 jours)

**Objectif** : Supporter les cas d'usage au-delà des applications.

1. **Templates de metadata** :
   - `application` — comportement actuel (un runtime, un entrypoint)
   - `agent` — modèle + prompt + runtime + toolcalls + capabilities réseau
   - `service` — multi-service avec orchestration
   - `plugin` — interface d'extension pour un hôte
2. **Multi-agent** : un seul binaire contenant plusieurs agents
   - `erebus build ./agents --entrypoint chat=chat.py --entrypoint code=code.py`
   - Le stub résout quel agent lancer selon `argv[1]`
   - Chaque agent a ses propres capabilities
3. **Exemples concrets dans `examples/`** :
   - `examples/python-agent/` — agent IA avec modèle locale + outils
   - `examples/multi-agent/` — 3 agents dans un seul binaire
   - `examples/microservice-cluster/` — 3 services avec orchestration
   - `examples/hugo-plugin/` — plugin Hugo statique
4. **Documentation** : guide de migration AppImage → erebus agents

---

## Priorité

1. **Phase 1** (stub → couches) — **bloque tout**, le système de couches est du code mort
2. **Phase 5** (security) — **nécessaire avant production**, capabilities sans enforcement = gimmick
3. **Phase 2** (hot-swap) — **différenciant agent IA**, remplacer un modèle sans rebuild
4. **Phase 3** (lazy loading) — **performance agent IA**, ne pas charger 7 Go de modèle au démarrage
5. **Phase 4** (registry) — **adoption**, partager des couches entre artefacts
6. **Phase 6** (templates) — **storytelling**, prouve que le format est universel

---

## Contraintes techniques

- **vfat** : le repo vit sur vfat (pas de bit exec). Les artefacts build vont dans `/tmp/erebus-stub-target`.
- **musl** : le stub compile avec `--target x86_64-unknown-linux-musl` pour un ELF statique.
- **CI** : clippy est lancé par crate, pas workspace-wide.
- **Edition** : Rust 2021, stable uniquement (pas de nightly).
- **Sécurité** : zero `unsafe` dans `erebus-core` et `erebus-cli`. Toute la logique unsafe est dans `erebus-stub`.
