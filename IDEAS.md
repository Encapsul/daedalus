# IDEES — Compilation complete

## 1. Position A : Universal Application Packaging (PRINCIPALE)

**Description** : daedalus packages any web, server, or CLI application into a single
self-extracting binary. The `.daedalus` format (`[stub][payload][metadata][footer]`)
contains the runtime + dependencies + code, signed Ed25519, with Sisir delta
updates and sandbox isolation (seccomp + Landlock). Supports 11 runtimes
(Python, Node.js, Deno, Java, Ruby, .NET/C#, Go, PHP, Perl, Hugo, Binary) across
Linux, macOS, and Windows.

### Pourquoi c'est le bon angle

- **Problème réel** : déployer une application nécessite Docker (daemon), un runtime
  installé (Python, Node, Go, etc.), ou un gestionnaire de paquets — l'installateur
  est souvent la source de bugs.
- **Solution** : un seul fichier exécutable. `cp app.daedalus /target/ && ./app.daedalus`
  → extraction en 2s, serveur prêt. Aucun runtime à installer sur la cible.
- **SISR** : les mises à jour ne téléchargent que les chunks modifiés (FastCDC),
  pas l'artefact complet — essentiel pour les grosses applications.
- Le concurrent direct (PyInstaller, Bun, AppImage) est mono-langage ou
  desktop-only ; daedalus est multi-runtime et multi-OS.

---

## 2. Position B : Compilateur vers formats existants (--to flag) — DEPRECATED

**Statut** : **Déprécié.** Abandonné au profit du format `.daedalus` universel.

**Raisons** :
- Dilution : le format `.daedalus` existant répond déjà au besoin de distribution
- L'OCI n'est concurrent qu'au-delà du réseau (pas dans le segment desktop/local)
- WASM/AppImage = features qui ne sont pas dans le segment prioritaire
- Le marché valorise la simplicité (single-file) + delta updates + multi-runtime

---

## 3. Analyse concurrencelle — segment application packaging

### Outils existants dans le segment universal packaging

| Outil | Format | Multi-runtime | SISR | Sandbox | Cross-OS |
|---|---|---|---|---|---|
| **PyInstaller** | Executable | Python only | None | None | Cross-OS |
| **Bun** | Single binary | JS/TS only | None | None | Cross-OS |
| **AppImage** | AppImage | Any (desktop) | zsync delta | None | Linux only |
| **Docker/BuildKit** | OCI image | Yes (any runtime) | zstd:chunked (partial) | Namespace | Cross-OS (w/ daemon) |
| **Nix** | Store | Yes | NAR deltas | Namespace | Cross-OS |
| **llamafile** | Fat C++ binary | C++ only | None | None | Cross-OS |
| **pkg** | Executable | Node only | None | None | Cross-OS |

## 4. Pourquoi daedalus contre les concurrents

**PyInstaller / Bun / pkg** = mono-langage. Un projet full-stack (Python backend +
Node frontend) nécessite deux outils. daedalus gère tout dans un seul.

| Aspect | PyInstaller + Bun + pkg | **daedalus** |
|---|---|---|
| Langues supportées | Python, JS/TS (separate tools) | **11 runtimes, un seul outil** |
| Runtime à installer | Oui (Python, Node, Go) | **Non — runtime embeddable** |
| Delta updates | None | **SISR content-defined delta** |
| Single file | Oui (mais mono-langue) | **Oui — multi-language** |
| Sandbox | None | **seccomp + Landlock** |
| Cross-arch | Rebuild | Cross-compilation via cargo zigbuild |
| Daemon requis | Non | **Non** |

---

## 5. Cas d'usage prioritaire : application universelle

Packager une application typique :

```
my-app/
  app.py            ← Flask/FastAPI/Django server (Python)
  requirements.txt  ← flask, requests, sqlalchemy
  package.json      ← frontend assets (Node frontend build)
  static/           ← assets
```

```bash
# Build une fois → un seul fichier
daedalus build ./my-app --embed-interpreter python3,nodejs -o myapp.daedalus

# Distribution → un seul fichier, marche partout
chmod +x myapp.daedalus && ./myapp.daedalus
# → Python runtime + deps + code + static assets extractés en 2s, server ready

# Mise à jour incrémentale → 15MB au lieu de 80MB
daedalus build ./my-app --update --enable-sisr
# → SISR delta : seuls les chunks modifiés sont téléchargés
```

---

## 6. Modèle économique — Plan cohérent 3 phases

### Phase 0 (0-3 mois) — Consulting + preuve de marché

- **Produit** : CLI open source (gratis) — `daedalus build`, `daedalus run`, `daedalus swap`
- **Service** : Consulting packaging + hardening sécurité (sécurité par background, pas par features)
- **Security testing sandbox** : distraire des PoCs/pentest dans un binary signé + sandboxé
- Chaque mission = feedback sur les features enterprise réellement valorisées
- Objectif : 2-3 clients payants → data pour productiser

### Phase 1 (3-9 mois) — Open core

Based on consulting feedback, ship enterprise features as paid tier:

| Tier | Price | Features |
|---|---|---|
| Free | $0 | Local build, single-file, sandbox basique |
| Pro | $7/user/mois | Private registry, SISR, Ed25519 signing |
| Enterprise | Sur mesure | Airgap, AES-256-GCM encryption, Landlock avancé, attestations, CI/CD plugins, support + SLA |

### Phase 2 (9-18 mois) — Scale

- Universal packaging = angle principal (YC positioning)
- Marketplace/registry **seulement si** la phase 1 prouve la demande
- IoT/embedded licensing **seulement si** le produit est adopté par des vendors

### Ce qu'on ne fait PAS

- **IA agents edge** — DEGACER'd, le packaging universel couvre déjà le besoin
- **Marketplace/registry** — chicken-and-egg, trop tôt
- **OCI/AppImage/WASM export** — deprecated (voir §2)

---

## 7. IoT / Edge Opportunity

Source: panorama terrain (cameras IP, Pi/SBC, routeurs, drones, edge AI, systèmes industriels, NAS/NVR).

### Où daedalus a du leverage

| Capacité actuelle | Problème IoT/edge résolu |
|---|---|
| Packager + runtime unique | Déployer apps sur Pi, edge devices, routeurs sans Docker |
| SHA-256 + Ed25519 signing | Intégrité pour devices à distance, audit, compliance |
| Delta updates (SISR) | Mise à jour de modèles/apps sur fleets avec connexions lentes |
| Seccomp + Landlock | Sandbox léger — apps isolées, pas de daemon requis |
| Multi-runtimes (Python/Node/Go/Binary) | Packaging d'outils variés (monitoring, agents, inference) |
| Cross-OS | Déployer sur Linux/macOS/Windows x64+ARM64 |

### Cas d'usage concrets (Phase 0 consulting)

1. **Edge appliance de remplacement** — Pi avec `.daedalus` apps qui remplacent
   les fonctions d'un routeur/switch/camera obsolète (VPN, firewall, NAT, monitoring).
2. **Outils de diagnostic/maintenance** — `.daedalus` tools packagés, sandboxés,
   signés, pour scanner des cameras (CVEs/config faibles), monitorer des routeurs
   dépréciés, backup des configs.
3. **Security tooling** — Avec ton background (Phrack/BH), packager des security tools
   en `.daedalus` signés : network scanners, firmware analyzers, config audit tools.
   Distribution fiable via signing + intégrité.

### Limite fondamentale — firmware ≠ application

`.daedalus` package des **applications** qui s'exécutent sur un OS existant, pas du firmware.
Flasher un firmware Cisco obsolète = impossible via daedalus. Solutions :
- Packager un **edge appliance** (Pi) qui remplace les fonctions du device deprecated
- Packager des **tools de diagnostic** qui tournent sur un OS adjacent
- Pour devices Linux embarqué (OpenWRT) : packager des apps qui s'exécutent dessus

### Features potentiellement à ajouter

| Problème | Feature | Effort |
|---|---|---|
| Fleet management (deploy, health, rollback) | Agent ou protocole léger | Moyen |
| Transfert USB / LAN sans internet | Support offline transfert (binary déjà autonome) | Faible |
| Capability templates par type de device | Templates prédéfinis (ex: "camera processing", "MQTT gateway") | Faible |
| Runtimes pré-compilés pour edge AI (Coral, Jetson) | Bundled runtimes spécialisés | Moyen |
