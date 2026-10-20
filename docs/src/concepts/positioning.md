# Positioning

## Customer profile

Un **développeur (solo ou petite équipe)** qui construit une **application web, serveur,
ou CLI** et veut la **distribuer** à des utilisateurs ou collègues qui n'ont rien installé -
ni Docker, ni Python, ni Node, ni quoi que ce soit. Il veut un fichier unique,
auto-extractible, qu'il peut signer pour prouver son origine, et mettre à jour
de manière incrémentale.

## Le segment : universal application packaging

| | Docker | PyInstaller/pkg | llamafile | **daedalus** |
|---|---|---|---|---|
| Runtime | Container daemon | Mono-language | C++ only | Plusieurs runtimes détectés. CI smoke : Python stdlib |
| Delta updates | Layer cache (partial) | None | None | SISR dans le code. Chiffre public = blob synthétique |
| Isolation | Namespace | None | None | User namespaces + denylist seccomp (18 syscalls) |
| Portable format | Image OCI | Executable | Single file | ELF Linux (`.daedalus`, alias `.de`) |
| Self-contained | Needs daemon | Runtime bundled | All-in-one binary | Stub + rootfs extrait dans `~/.cache` |

### Qui a ce problème aujourd'hui?

1. **Développeurs d'apps web multi-runtime** : publient un repo avec `requirements.txt` + `package.json`,
   mais leurs utilisateurs doivent installer Python + Node + toutes les dépendances. Setup : 30+ minutes.

2. **Startups SaaS** : construisent une API avec Python + un serveur FastAPI, mais le déploiement sur
   des serveurs edge heterogènes (x64/ARM64, Linux/macOS/Windows) nécessite des builds par architecture.

3. **Équipes de prototypage** : veulent partager une app expérimentale entre collègues sans créer
   une image Docker (trop lourd), sans PyInstaller (mono-language), sans setup manuel.

## Pourquoi maintenant?

- **Multi-language apps sont la norme** : un projet moderne combine Python backend + Node frontend.
  PyInstaller gère Python seulement, Bun gère JS/TS seulement - il faut deux outils.
- **llamafile (Mozilla-Ocho, 25k★)** : le concurrent direct, mais C++ only, pas de delta updates,
  pas de sandbox, pas de multi-runtime.
- **Docker est trop lourd pour le laptop** : daemon, root, ressources. Une app devrait démarrer
  en 2 secondes, pas nécessiter un container engine.
- **Déploiement multi-arch** : cross-compilation fragile, gestion manuelle des runtimes par architecture.

## Pourquoi daedalus?

### Against llamafile
llamafile = un binaire C++ qui package un modèle + runtime llama.cpp. C'est fait pour tourner
un modèle, pas pour packager une application complète avec serveur web + API + runtime.

daedalus = packager plusieurs runtimes détectés (Python, Node, Go, binaire) dans un ELF Linux.
Les mises à jour SISR existent dans le code. Le chiffre « ~90 % » publié plus tôt est une
mesure synthétique (200 Mio, 10 % d'octets changés), pas une paire de poids réelle.
L'isolation Linux est un denylist seccomp d'une vingtaine de syscalls plus des user
namespaces quand le noyau les autorise. Ce n'est pas un profil d'application.

### Against Docker
Docker est fait pour l'orchestration, pas pour la distribution locale. Besoin d'un daemon,
de root (ou de user namespaces), de network pour pull l'image.

daedalus = un fichier exécutable (`./app.daedalus`), extraction atomique, fonctionne sans daemon
et sans privilèges root (user namespaces).

### Against PyInstaller/pkg
Mono-language : PyInstaller marche pour Python, pkg pour Node. Mais une app moderne a souvent
besoin de **plusieurs langages** : un backend Python/Node, un worker Go, un binaire natif.

daedalus = multi-runtime dans un seul format. Une app peut avoir un runtime Python pour l'API,
un binaire C++ pour un moteur, et un script Node pour un worker - le tout dans un `.daedalus`.

## AI-native deployment

| | Cloud API | llamafile | **daedalus** |
|---|---|---|---|
| Runtime | Remote API | C++ only | Runtimes détectés, artefact ELF Linux |
| Model updates | Full redownload | Full redownload | SISR si les poids sont dans l'artefact |
| Offline capable | No | Yes, model in the file | Seulement si le build a embarqué les poids |
| Data sovereignty | No | Model runs locally | Clé privée Ed25519 hors du fichier. Clé AES optionnelle dedans |

### The problem with cloud-first AI

According to the ITU, an estimated 6 billion people - about three-quarters of the world's population - are using the Internet in 2025. However, 2.2 billion people remain offline, 96% of them in low- and middle-income countries. In low-income countries, only 14% of rural residents are online. 5G covers 55% of the global population, but only 4% in low-income countries.

For AI deployment, this means:
- **2.2 billion people** cannot access cloud-based AI services
- **Rural/isolated sites** have no reliable path to cloud inference
- **Regulated industries** (healthcare, finance, defense) cannot send sensitive data to external APIs
- **Bandwidth costs** make full model downloads prohibitive on metered connections

### Why local AI needs daedalus

The edge AI hardware market is projected to reach $36.2 billion in 2026, with inference chips dominating at 86.5% of activity. By 2026, an estimated 80% of AI inference is expected to happen locally on devices rather than in cloud data centers.

But shipping a local AI app is hard:
- The app + runtime + model must be bundled together
- Model updates over low-bandwidth links require delta compression
- The binary must be signed and verified for security
- Multiple runtimes (Python backend, Node frontend, Ollama model) must work together

Shipping a local model is the niche wedge. The clinic example still does not
do it: `examples/offline-health-agri` has a `Modelfile` and no weights.
`diagnose` prints hardcoded text, then calls Ollama on localhost if one is
already running. Use `examples/local-model-kit` when you want real weights:
`fetch_weights.sh` pulls a Gemma 3 270M Q4 GGUF from Hugging Face into
gitignored `models/`; a `.daedalus` contains those bytes only if the file was
present at build time. Do not call a kit without the GGUF "offline AI".

What the tool can do once the weights are actually in the tree:

- Detect Ollama or Gemma layouts (`Modelfile`, `models/*.gguf`)
- Carry those bytes in the payload, same as any other file
- Apply a signed SISR delta later. Measure it with
  `DAEDALUS_SISR_MODEL_V1` / `DAEDALUS_SISR_MODEL_V2` and the ignored test
  `gemma_weight_delta_bandwidth`. Do not quote the synthetic 89.7% figure as
  a model result

The ITU and edge-AI market figures above are not daedalus usage.

### Sources

- ITU Facts and Figures 2025: https://www.itu.int/itu-d/reports/statistics/global-connectivity-report-2025/
- Axis Intelligence / Grand View Research Edge AI Market 2026: https://axis-intelligence.com/edge-ai-statistics/
- Presenc AI Ollama Ecosystem State 2026: https://presenc.ai/research/ollama-ecosystem-state-2026
- Ertas.ai Edge AI in 2026: https://www.ertas.ai/blog/edge-ai-local-inference-2026

## Ce daedalus n'est PAS un nouveau format

Nous ne créons pas un nouveau standard. Le format `.daedalus` est :
- Un **ELF valide** (Linux) - `chmod +x && ./app.daedalus` fonctionne
- Un **payload tar/zstd ou SquashFS** - décompressible avec `tar`/`unsquashfs`
- Des **signatures Ed25519 standards** - vérifiables avec la librarie crypto standard
- Un **rootfs POSIX** - pas de kernel features propriétaires

Le format existe depuis v2 (plain) → v3 (signed) → v4 (encrypted) → v5 (squashfs),
et lit les versions anciennes. Nous ne forkons jamais le format.

## Ce daedalus n'est PAS

- **Pas un Docker killer** : Docker reste pour l'orchestration et multi-container.
- **Pas une VM** : on ne virtualise pas le kernel; l'app tourne sur le kernel host.
- **Pas un package manager** : pas de registry centrale requise (optionnelle).
- **Pas desktop packaging** : AppImage/Snap/Flatpak gèrent l'intégration desktop (icônes,
  menus, X11/Wayland). daedalus cible **headless web/server apps** - "je lance le binaire,
  un serveur démarre, j'ouvre mon navigateur."
- **Pas un format inter-language bridge** : daedalus ne compile pas du Python vers du WASM.
  Chaque runtime marche dans son propre environnement (Python/venv, Node/node_modules, Go/binary).

## The most promising use case

Distribuer une appli dont le runtime n'est pas sur la machine cible, comme un
ELF Linux. Le cas multi-runtime (FastAPI + Node + un binaire Go dans un seul
fichier) est le différenciateur vis-à-vis de pkg et PyInstaller. Il n'a pas
encore de smoke CI. Le seul smoke CI est `examples/hello-web`.

Le delta « 15 Mo au lieu de 80 Mo » n'est pas une mesure de ce dépôt. La seule
mesure SISR publiée ici est synthétique (200 Mio, 10 % d'octets changés,
89,7 %). Docker reste plus simple dès qu'il y a plusieurs processus à
orchestrer. AppImage et pkg gagnent le cold start sur un hello-node.
