# Positioning

## Customer profile

Un **développeur (solo ou petite équipe)** qui construit une **application web, serveur,
ou CLI** et veut la **distribuer** à des utilisateurs ou collègues qui n'ont rien installé —
ni Docker, ni Python, ni Node, ni quoi que ce soit. Il veut un fichier unique,
auto-extractible, qu'il peut signer pour prouver son origine, et mettre à jour
de manière incrémentale.

## Le segment : universal application packaging

| | Docker | PyInstaller/pkg | llamafile | **daedalus** |
|---|---|---|---|---|
| Runtime | Container daemon | Mono-language | C++ only | **Multi-runtime** |
| Delta updates | Layer cache (partial) | None | None | **SISR content-defined** |
| Sandbox | Namespace | None | None | **seccomp + Landlock** |
| Portable format | Image OCI | Executable | Single file | **Single .daedalus file** |
| Self-contained | Needs daemon | Runtime bundled | All-in-one binary | **Stub + rootfs + app** |

### Qui a ce problème aujourd'hui?

1. **Développeurs d'apps web multi-runtime** : publient un repo avec `requirements.txt` + `package.json`,
   mais leurs utilisateurs doivent installer Python + Node + toutes les dépendances. Setup : 30+ minutes.

2. **Startups SaaS** : construisent une API avec Python + un serveur FastAPI, mais le déploiement sur
   des serveurs edge heterogènes (x64/ARM64, Linux/macOS/Windows) nécessite des builds par architecture.

3. **Équipes de prototypage** : veulent partager une app expérimentale entre collègues sans créer
   une image Docker (trop lourd), sans PyInstaller (mono-language), sans setup manuel.

## Pourquoi maintenant?

- **Multi-language apps sont la norme** : un projet moderne combine Python backend + Node frontend.
  PyInstaller gère Python seulement, Bun gère JS/TS seulement — il faut deux outils.
- **llamafile (Mozilla-Ocho, 25k★)** : le concurrent direct, mais C++ only, pas de delta updates,
  pas de sandbox, pas de multi-runtime.
- **Docker est trop lourd pour le laptop** : daemon, root, ressources. Une app devrait démarrer
  en 2 secondes, pas nécessiter un container engine.
- **Déploiement multi-arch** : cross-compilation fragile, gestion manuelle des runtimes par architecture.

## Pourquoi daedalus?

### Against llamafile
llamafile = un binaire C++ qui package un modèle + runtime llama.cpp. C'est fait pour tourner
un modèle, pas pour packager une application complète avec serveur web + API + runtime.

daedalus = packager **n'importe quel runtime** (Python, Node, Go, Binary) + code + config
dans un format unifié, avec **delta updates Sisir** (llamafile télécharge le fichier entier à
chaque mise à jour) et **sandbox seccomp+Landlock** (llamafile n'a pas d'isolation).

### Against Docker
Docker est fait pour l'orchestration, pas pour la distribution locale. Besoin d'un daemon,
de root (ou de user namespaces), de network pour pull l'image.

daedalus = un fichier exécutable (`./app.daedalus`), extraction atomique, fonctionne sans daemon
et sans privilèges root (user namespaces).

### Against PyInstaller/pkg
Mono-language : PyInstaller marche pour Python, pkg pour Node. Mais une app moderne a souvent
besoin de **plusieurs langages** : un backend Python/Node, un worker Go, un binaire natif.

daedalus = multi-runtime dans un seul format. Une app peut avoir un runtime Python pour l'API,
un binaire C++ pour un moteur, et un script Node pour un worker — le tout dans un `.daedalus`.

## Ce daedalus n'est PAS un nouveau format

Nous ne créons pas un nouveau standard. Le format `.daedalus` est :
- Un **ELF valide** (Linux) — `chmod +x && ./app.daedalus` fonctionne
- Un **payload tar/zstd ou SquashFS** — décompressible avec `tar`/`unsquashfs`
- Des **signatures Ed25519 standards** — vérifiables avec la librarie crypto standard
- Un **rootfs POSIX** — pas de kernel features propriétaires

Le format existe depuis v2 (plain) → v3 (signed) → v4 (encrypted) → v5 (squashfs),
et lit les versions anciennes. Nous ne forkons jamais le format.

## Ce daedalus n'est PAS

- **Pas un Docker killer** : Docker reste pour l'orchestration et multi-container.
- **Pas une VM** : on ne virtualise pas le kernel; l'app tourne sur le kernel host.
- **Pas un package manager** : pas de registry centrale requise (optionnelle).
- **Pas desktop packaging** : AppImage/Snap/Flatpak gèrent l'intégration desktop (icônes,
  menus, X11/Wayland). daedalus cible **headless web/server apps** — "je lance le binaire,
  un serveur démarre, j'ouvre mon navigateur."
- **Pas un format inter-language bridge** : daedalus ne compile pas du Python vers du WASM.
  Chaque runtime marche dans son propre environnement (Python/venv, Node/node_modules, Go/binary).

## The most promising use case

Distributing **multi-runtime web applications** : packager un serveur FastAPI (Python) +
un bundler Node (frontend) + un binaire Go (worker), dans un seul fichier qui démarre
avec `./app.daedalus` et qui se met à jour via Sisir delta updates (15MB au lieu de 80MB).

Aujourd'hui ce cas a pas de solution clean :
- Docker : trop lourd, daemon requis, pas fait pour laptop
- AppImage : pas fait pour serveur headless
- PyInstaller/pkg : mono-language, ne gère pas multi-runtime
