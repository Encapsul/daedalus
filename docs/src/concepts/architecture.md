# Architecture

`xbin` est découpé en 4 couches avec des interfaces claires entre elles, pour
qu'on puisse faire évoluer chaque couche (ex : passer de l'extraction tar à
squashfs+mmap) sans tout réécrire.

```
┌──────────────────────────────────────┐
│            CLI (xbin)                 │  build · run · inspect · clean
├──────────────────────────────────────┤
│            Builder                    │  Analyzer + Packager
│   ┌──────────────┬──────────────┐     │
│   │   Analyzer   │   Packager   │     │
│   └──────────────┴──────────────┘     │
├──────────────────────────────────────┤
│            Runtime (launcher)         │  self-read · cache · executor
│   ┌──────────────┬──────────────┐     │
│   │    Cache     │   Executor   │     │
│   └──────────────┴──────────────┘     │
├──────────────────────────────────────┤
│            Format .xbin               │  spec binaire partagée
└──────────────────────────────────────┘
```

Chaque couche ne connaît que celle du dessous. Le **format `.xbin`** est le
contrat partagé : le builder (Python) l'écrit, le launcher (Rust) le lit. Tant
que le format est respecté, les deux côtés évoluent indépendamment.

## Le schéma d'architecture (esquisse initiale)

Voici l'architecture telle qu'esquissée au départ du projet. C'est **une
architecture cible possible** — elle décrit l'état visé en fin de Phase 2/3, pas
l'état actuel du MVP.

![Architecture xbin](../images/architecture.png)

### Lecture du schéma

Le schéma se lit de haut en bas, comme un flux :

1. **CLI** (`xbin ./my_app`) — quatre commandes : *Build · Run · Inspect ·
   Clean*. C'est la surface utilisateur.

2. **Builder** — deux sous-composants :
   - **Analyzer** : `ldd` récursif sur les `.so`, détection du runtime
     (python/node/binaire), et détection des dépendances cachées
     (sous-processus, `dlopen`) — voir l'annotation *AI* à droite.
   - **Packager** : construit le rootfs, compresse en zstd, assemble le `.xbin`
     final.
   - L'annotation **AI** (en haut à droite) marque le rôle ciblé de l'IA :
     *analyser le code source, détecter les sous-processus et les `dlopen`
     invisibles à `ldd`*. C'est la différenciation du projet — voir
     [Détection des dépendances](../guides/dependances.md).

3. **`.xbin` Format** — la couche centrale : *ELF Launcher · Payload zstd ·
   Metadata JSON · Magic + SHA-256*. C'est le format de fichier décrit en
   [référence](../reference/format.md).

4. **Runtime** — deux sous-composants :
   - **Cache** : `~/.cache/xbin/{sha256}/`, extraction unique, `flock()` pour
     l'accès concurrent, nettoyage LRU.
   - **Executor** : *user namespaces Linux · pivot_root (isolation) · filtre
     seccomp · fallback LD_LIBRARY_PATH*.

5. **Objectif** (en bas) : *single binaries · warm start < 100 ms · 0
   dépendance*.

### Écart entre le schéma cible et le MVP actuel

Le schéma décrit l'**ambition**. Voici honnêtement où on en est :

| Élément du schéma | Cible | MVP actuel |
|---|---|---|
| CLI build/run/inspect | ✅ | ✅ implémenté |
| Analyzer `ldd` + runtime | ✅ | ✅ implémenté |
| Analyzer IA (deps cachées) | ✅ | ⏳ Phase 2 |
| Format ELF + zstd + meta + SHA-256 | ✅ | ✅ implémenté |
| Cache `{sha256}` + extraction atomique | ✅ | ✅ implémenté (le `flock()` arrive) |
| Executor `LD_LIBRARY_PATH` (niveau 0) | ✅ | ✅ implémenté |
| Executor user namespaces + pivot_root + seccomp | ✅ | ⏳ Phase 2 |
| Signature Ed25519 | ✅ | ⏳ Phase 2 |
| warm start < 100 ms | ✅ | ⏳ (limité aujourd'hui par le boot du runtime embarqué, pas par xbin) |

> **Note de conception.** Le schéma place l'isolation (namespaces, seccomp) au
> cœur du runtime. En pratique on a choisi de **démarrer au niveau 0**
> (`LD_LIBRARY_PATH`, aucune isolation) parce que l'isolation est une *feature*,
> pas le cœur de la proposition de valeur. La valeur, c'est *« un fichier, ça
> tourne »*. L'isolation se monte en puissance ensuite, sans changer le format.

## Le flux complet en une image

```
MACHINE DE DEV                          MACHINE CIBLE

mon_app/
  app.py
  requirements.txt   →  xbin build  →   mon_app.xbin   →  ./mon_app.xbin  →  ça tourne
+ python3                              (1 fichier)
+ libs .so
```

Le `xbin build` fait le travail difficile **une fois**. L'utilisateur final ne
voit qu'un fichier simple.
