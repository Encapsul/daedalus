# x.bin — ROADMAP

## Vision (Mise à jour)

x.bin n'est plus seulement un packager d'applications.  
C'est un **format d'artefact exécutable universel**, capable de transporter :

- une application classique ✅
- un agent IA (modèle + prompt + runtime) ✅  
- un service/microservice ✅
- un plugin/extension ✅

> Le format est conçu comme une unité autonome = stub + runtime + payload + metadata + signature.
> Rien n'est hérité du système hôte.

---

## État actuel

✅ CAS (`xbin-core/src/cas.rs`)  
✅ Format binaire (`xbin-core/src/format.rs`)  
✅ Metadata riche (`xbin-core/src/metadata.rs`)  
✅ Signature Ed25519 (`stub/src/crypto.rs`)  
✅ Stub runtime (`stub/src/main.rs`)  
✅ CLI (`xbin-cli/`)  
✅ SISR/delta updates (`xbin-core/src/sisr/`)

---

## Prochaines étapes (Roadmap refactor)

### Phase 1 : Abstraction du runtime (2-3 jours)

**Objectif** : Transformer le runtime en couche composable.

1. **Créer `trait Layer`** dans `xbin-core`
   - méthodes : `name()`, `kind()`, `payload_sha256()`

2. **Créer `trait Entrypoint`** dans `xbin-core`
   - méthode : `execute(&self, ctx: &Context) -> io::Result<()>`

3. **Refactoriser `detect.rs`** pour exposer un `Entrypoint` par runtime supporté (Python, Node, Go, WASM, etc.)

---

### Phase 2 : Graphe de blobs + metadata générique (3-4 jours)

**Objectif** : Rendre le manifeste extensible à n'importe quel artefact.

1. **Mettre à jour `Metadata`** pour contenir :
   - `Vec<Layer>` (au lieu de `runtime`, `entrypoint`, `services` en dur)
   - `Entrypoint` (le point d'entrée principal)

2. **Faire de `assemble_xbin`** → builder générique de graphe de layers

3. **Mettre à jour `stub`** pour parcourir les layers dynamiquement

---

### Phase 3 : Registry CAS (4-5 jours)

**Objectif** : Publier/charger des layers/artefacts via un registre.

1. **Implémenter `Registry`** dans `xbin-core`
   - `pub fn push_layer(&self, layer: &Layer) -> Result<()>`
   - `pub fn pull_layer(&self, hash: &str) -> Result<Layer>`
   - `pub fn publish_artifact(&self, artifact: &Artifact) -> Result<String>`

2. **Serveur registry simple** (actuel, ou intégrer à `xbin-cli`)

3. **CLI `xbin registry push/pull/list`**

---

### Phase 4 : Permissions & sécurité runtime (3 jours)

**Objectif** : Contrôler ce que peut faire chaque layer.

1. **Ajouter `Capability` enum** :
   - `ReadFile(path)`
   - `WriteFile(path)`
   - `Network`
   - `Exec`

2. **Associer capabilities aux layers dans le manifeste**

3. **Application dans le stub (Landlock/secrets)**

---

### Phase 5 : Support de nouveaux artefacts (3-4 jours)

**Objectif** : Packaging non seulement d'applications mais d'agents/services.

1. **Templates de metadata** :
   - `application`
   - `agent`
   - `service`
   - `plugin`

2. **Exemples concrets dans `examples/`**

3. **Docs migration AppImage → xbin agents**

---

## Priorité

1. Phase 1 (abstraction) — **bloque tout le reste**
2. Phase 2 (graphe) — **nécessaire pour extensibilité**
3. Phase 4 (permissions) — **problème de sécurité si non fait**
4. Phase 3 (registry) — **UX + adoption**
5. Phase 5 (templates) — **politique / storytelling**

---

## Priorité absolue : SECURITY-AUDIT.md

Avant tout développement, un audit complet est requis. Voir `SECURITY-AUDIT.md`.
