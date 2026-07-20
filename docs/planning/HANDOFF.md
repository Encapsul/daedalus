# HANDOFF — état de la session pour reprise par un autre modèle

> Document de passation. Lis-le en entier avant de continuer. Il capture le
> contexte, les décisions, ce qui marche, et la suite. Date : 2026-06-23.

## 0. Contexte humain

L'utilisateur (carmeldjegui01@gmail.com) a une idée de produit, `xbin` /
**x.bin**, documentée dans des fichiers à la racine (`xbin-project.md`,
`Startup.idea`, `avis-et-change.md`, images `xbin-arch-x*.png`). Il a demandé :
1. un avis « investisseur YC » sur l'idée (donné — voir plus bas) ;
2. de **commencer à coder le projet maintenant** ;
3. la doc en **mdbook** + un bon README ;
4. d'analyser les images comme « architecture cible possible » ;
5. du **code documenté et compréhensible** (commentaires en français) ;
6. le **support des dépendances Python** ;
7. le **rebuild incrémental** (« update directement dans le binaire ») ;
8. le naming : marque **x.bin**, commande `xbin` (décidé).

Style de réponse attendu : direct, honnête, en français, sans flatterie creuse.

## 1. Ce qu'est x.bin (le produit)

Transforme une app **web/serveur/CLI headless** (pas desktop GUI) en **un seul
exécutable autonome**. `chmod +x app.xbin && ./app.xbin` → le serveur démarre.
Zéro runtime à installer, zéro Docker.

**Positionnement clé (différenciation) :**
- PAS un concurrent d'AppImage/Snap/Flatpak (eux = desktop GUI).
- PAS `pkg`/`nexe`/PyInstaller (eux = mono-langage). x.bin est language-agnostic
  (empaquette un *rootfs*).
- Angle fort identifié : distribution de **modèles IA locaux** (llama.cpp + modèle
  + serveur en un fichier), et détection IA des deps cachées (`subprocess`,
  `dlopen`) qu'aucun outil statique ne voit.
- La question à toujours savoir répondre : *« pourquoi pas AppImage/Docker/pkg ? »*

## 2. Architecture (4 couches)

```
CLI (xbin)         build · run · inspect · clean        [cli/xbin/cli.py]
Builder            Analyzer (ldd+runtime) · Packager    [cli/xbin/build.py, analyzer/]
Format .xbin       contrat partagé builder<->launcher   [format.py + format.rs]
Runtime (launcher) self-read · cache · executor         [stub/src/main.rs]
```

- **Launcher** : Rust, compilé **statique musl** (`x86_64-unknown-linux-musl`),
  ~615 KB. Pur-Rust (pas de toolchain C) : `ruzstd` (décompression), `tar`,
  `sha2`, `serde_json`. Décompression seulement côté launcher ; compression côté
  builder via le CLI `zstd`.
- **Builder/CLI** : Python 3, **stdlib pure** (argparse, pas de click/rich) pour
  zéro friction d'installation.

## 3. Format `.xbin` (IMPORTANT)

Un ELF valide (launcher) à l'offset 0 + données collées + **footer 84 bytes en
fin de fichier**. Le magic ELF DOIT être à l'offset 0 (le doc initial de l'user
mettait le magic au début — c'est FAUX, corrigé). Le launcher se lit via
`/proc/self/exe`, seek depuis la fin, lit le footer.

Footer 84 bytes (little-endian), spec partagée **format.py ⇄ format.rs** (garder
synchro) : magic `XBIN\x01`(5) + version(1) + arch(1) + flags(1) +
payload_offset(8) + payload_csize(8) + payload_usize(8) + payload_sha256(32) +
meta_offset(8) + meta_size(8) + footer_magic `0xBEEFCAFE`(4).

**Format v2 (actuel) = couches.** Layout : `[stub][couche runtime][couche app][metadata][footer]`.
- Footer v2 : `payload_offset`=début région couches, `payload_csize`=taille totale
  couches, `payload_usize`=0, `payload_sha256`=SHA-256(**couches ‖ metadata**).
- La **table des couches** est dans le metadata JSON : `layers:[{kind,offset,csize,usize,sha256}]`
  (offset = absolu dans le fichier ; sha256 = du blob compressé, sert de clé de cache).
- Launcher gère v1 (payload mono) ET v2 (multi-couches) — branche sur
  `format_version >= 2 && !layers.empty()`.

Métadonnées JSON : `name, xbin_version, created, runtime, isolation, entrypoint
(argv, chemins absolus relatifs au rootfs), env, cwd, layers`.

**Token `${ROOTFS}`** : dans `env`, le builder écrit p.ex.
`PYTHONPATH=${ROOTFS}/app/site-packages` ; le launcher remplace `${ROOTFS}` par
le chemin réel du rootfs cache à l'exécution (le builder ne le connaît pas).

## 4. Ce qui MARCHE (testé end-to-end aujourd'hui)

- `xbin build <dir>` → `.xbin` exécutable ; `run` ; `inspect` (montre les
  couches) ; `clean` (préserve le cache de build, `--all` le vide).
- **Python stdlib** : exemple `examples/hello-web/` (serveur http.server).
- **Deps Python tierces** : exemple `examples/bottle-web/` — `bottle` (framework
  web) vendu dans `examples/bottle-web/site-packages/bottle.py`, chargé via
  `PYTHONPATH=${ROOTFS}/app/site-packages`. Sert du HTTP. ✅
- **Détection deps** : `ldd` sur interpréteur + `.so` des site-packages + binaire
  natif → libs embarquées dans la couche runtime.
- **Cache d'extraction** : `~/.cache/xbin/{clé}/rootfs`, extraction atomique
  (`rename()`), `flock()` pour la concurrence, marqueur `.ready`.
- **Rebuild incrémental (v2)** : couche runtime (interpréteur+stdlib+.so+/etc)
  séparée de la couche app (code+site-packages). Cache de build
  `~/.cache/xbin/build/{hash}.zst`. **Mesuré : build initial ~25 s → rebuild
  ~1.2 s** quand seul le code change ; deux apps au même runtime partagent la
  couche (2e app ~1 s). tar déterministe (mtime/uid/gid=0, trié) pour hash stable.

## 5. Build & test (commandes)

```bash
# prérequis : rustup target add x86_64-unknown-linux-musl ; python3 ; zstd ; ldd
make stub      # compile le launcher (stub/target/.../xbin-stub)
make example   # build examples/hello-web -> hello-web.xbin
make docs      # mdbook build (docs/book/) ; make docs-serve pour live
# CLI directe (pas d'install) :
cd cli && PYTHONPATH=. python3 -m xbin build ../examples/bottle-web -o ../bottle-web.xbin
cd cli && PYTHONPATH=. python3 -m xbin inspect ../bottle-web.xbin
XBIN_VERBOSE=1 PORT=8080 ./bottle-web.xbin   # XBIN_VERBOSE montre cold/warm start
```

Environnement de la session : Linux Mint x86_64, Rust 1.96, Python 3.12,
`unprivileged_userns_clone=1` (userns dispo pour Phase 2). **Pas de pip** global
(ensurepip absent — split Debian), **pas de node**. Réseau OK.

## 6. Carte des fichiers

```
stub/src/main.rs      launcher : run(), extract_atomic (multi-couches),
                      exec_app (argv/env, ${ROOTFS}), flock_exclusive, cache_key_v2,
                      slice_layers. extern execve+flock (pas de crate nix).
stub/src/format.rs    Footer (lecture), FORMAT_VERSION=2.
cli/xbin/cli.py       argparse : build/run/inspect/clean.
cli/xbin/build.py     find_stub, _copy_into_rootfs, _build_runtime_layer,
                      _build_app_layer, _tar_deterministic, _zstd,
                      _build_cache_dir, _compress_layer_cached, build().
cli/xbin/format.py    Footer (pack/unpack), FORMAT_VERSION=2. SYNC avec format.rs.
cli/xbin/inspect.py   affiche couches + integrity.
cli/xbin/clean.py     cache d'extraction (préserve build/), --all.
cli/xbin/analyzer/runtime.py  detect() python/node/binary, RuntimePlan,
                      _find_site_packages (.venv/venv/site-packages/).
cli/xbin/analyzer/ldd.py      shared_libs() via ldd.
examples/hello-web/   app.py (stdlib http.server).
examples/bottle-web/  app.py + requirements.txt + site-packages/bottle.py.
docs/                 mdbook (book.toml + src/SUMMARY.md + chapitres). title "x.bin".
Makefile, README.md, .gitignore
```

## 7. Décisions de conception prises

- Isolation : **on démarre au niveau 0** (`LD_LIBRARY_PATH`, pas de chroot).
  L'isolation (chroot/userns/seccomp) est une *feature*, pas le cœur. Niveaux
  1/2 = Phase 2. Le format ne change pas quand on montera en isolation (champ
  `isolation` + comportement launcher).
- Launcher en Rust/musl pur-Rust (sécurité mémoire + portabilité statique).
- Builder en Python stdlib (rapidité de dev, futur analyzer IA s'y branche).
- Format versionné + footer fixe → évolutif sans casser (v1 et v2 coexistent).
- Signature Ed25519 = Phase 2 (le footer a déjà `flags` bit0=signé réservé ; la
  sécurité doc décrit la séquence : vérifier signature AVANT extraction).

## 8. Limites connues (à dire honnêtement)

- **Portabilité inter-distribution** non garantie au niveau 0 : l'ELF de
  l'interpréteur garde son interpreter path `/lib64/ld-linux…` résolu sur l'hôte.
  Vraie portabilité = niveau 2 (userns + pivot_root, où ld-linux résout dans le
  rootfs). Aujourd'hui : marche sur machine compatible (même famille glibc).
- **`requirements.txt` sans venv** : pas encore de pip-install au build (pip
  cassé sur cette machine de toute façon). Aujourd'hui : fournir `.venv` ou
  `site-packages/` vendu.
- **Node** : détecté (`runtime.py`) mais pas testé end-to-end (node absent).
- **Réutilisation d'extraction par couche** côté cible : pas encore (un changement
  de couche app ré-extrait tout). Le gain au *build* est lui acquis. Nécessite
  overlayfs (niveau 2) pour superposer sans re-extraire.

## 9. Prochaines étapes proposées (l'user choisira)

Par ordre de valeur :
1. **`requirements.txt` → pip install au build** (venv temporaire) : complète
   Python. Attention : pip indisponible sur la machine de session.
2. **Node.js de bout en bout** : prouve le language-agnostic (node + node_modules).
3. **Signature Ed25519** : footer v2→v3 avec bloc signature, `xbin keygen/sign/verify`,
   vérif avant extraction, trust model (`$XDG_DATA_HOME/xbin/trusted-keys/`).
4. **Isolation niveau 2** (userns + pivot_root + seccomp) : portabilité réelle +
   réutilisation d'extraction par couche via overlayfs.
5. **Analyzer IA** : `--ai-analyze` → génère `xbin.toml` (deps cachées). C'est la
   différenciation produit. Utiliser le SDK Claude (modèles récents : Opus 4.8
   `claude-opus-4-8`, Sonnet 4.6 `claude-sonnet-4-6`).

## 10. Avis YC déjà donné (résumé, pour cohérence du discours)

Doc technique excellente. Risques/objections à toujours adresser : AppImage
existe (réponse : desktop vs web headless), le marché se résout par en bas
(Go/Rust statiques, Node SEA, PyInstaller — réponse : language-agnostic +
headless + IA deps), glibc est l'enfer (réponse : niveau 2). Meilleur beachhead :
**distribution de modèles IA locaux**. Pitch : *« Homebrew/Docker-léger pour les
apps IA locales »*.

## 11. Règles de travail

- Ne JAMAIS committer/push sans demande explicite de l'user (repo `git init` fait,
  rien de commité). Si commit demandé : message en fin avec
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- Garder **format.py et format.rs synchronisés** à tout changement de format.
- Recompiler le stub (`make stub`) après toute modif Rust AVANT de rebuild un
  `.xbin` (le builder embarque le stub compilé).
- Code commenté en français, lisible, sans sur-ingénierie.
