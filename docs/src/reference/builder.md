# Le builder

Le builder analyse une application et produit le `.xbin`. Il est écrit en
**Python** (stdlib pure pour le MVP — zéro friction d'installation).

- **Code** : `cli/xbin/build.py`, `cli/xbin/analyzer/`
- **Pourquoi Python** : le builder est de la logique métier (parcourir des
  répertoires, appeler `ldd`, manipuler des chemins, assembler des bytes). Python
  est rapide à écrire et à modifier, et le futur Analyzer IA s'y branche
  naturellement.

## Les trois étapes

### 1. Analyse — `analyzer/`

- **`runtime.py`** détecte le runtime et résout l'entrypoint :
  - `app.py` / `main.py` / `server.py` → **python** ;
  - `package.json` → **node** ;
  - un seul exécutable ELF → **binaire natif**.
  - Renvoie un `RuntimePlan` : interpréteur à embarquer, entrypoint (relatif au
    rootfs), `cwd`, `env`, dossiers supplémentaires (ex : la stdlib Python).
- **`ldd.py`** liste les `.so` nécessaires via `ldd` (qui résout déjà
  transitivement) et inclut le dynamic loader `ld-linux`.

### 2. Construction du rootfs — `_build_rootfs()`

Assemble un mini-filesystem qui contient **exactement** le nécessaire :

```
rootfs/
  app/                          ← le code de l'application
  usr/bin/python3.12            ← l'interpréteur
  usr/lib/python3.12/           ← la stdlib
  usr/lib/x86_64-linux-gnu/     ← les .so (libc, etc.)
  lib64/ld-linux-x86-64.so.2    ← le dynamic loader
  etc/{passwd,group,resolv.conf}← config minimale
```

Point clé : on **préserve l'arborescence absolue** des fichiers copiés
(`/usr/lib/...` → `rootfs/usr/lib/...`). C'est ce qui permet à l'interpréteur
Python embarqué de retrouver sa stdlib par détection de *landmark* relative à son
propre chemin.

### 3. Découpage en couches + compression — `build()`

Le builder construit **deux couches** séparées (format v2) :

- **couche runtime** (`_build_runtime_layer`) : interpréteur + stdlib + `.so` +
  `/etc`. Indépendante du code de l'app.
- **couche app** (`_build_app_layer`) : code de l'app + site-packages. Petite et
  volatile.

Chaque couche est `tar`-isée de façon **déterministe** (`_tar_deterministic` :
mtime/uid/gid normalisés, entrées triées) pour que *même contenu → mêmes bytes →
même hash*. Puis compressée en `zstd -19`.

**Cache de build** (`~/.cache/xbin/build/{hash}.zst`) : la couche runtime est
recherchée par le hash de son tar. Si un blob identique existe déjà, il est
**réutilisé sans recompression** — c'est ce qui rend les rebuilds (et les builds
d'apps partageant le même runtime) quasi instantanés.

Assemblage final, puis `chmod +x` :

```
[ stub ELF ][ couche runtime ][ couche app ][ metadata JSON ][ footer 84B ]
^0          ^payload_offset                  ^meta_offset      ^EOF-84
```

Voir [Format .xbin](./format.md#les-couches-v2) pour le détail de la table des
couches.

## Sortie typique

Premier build (cache de build froid) :

```
$ xbin build ./examples/bottle-web
[xbin] building 'bottle-web'
  runtime: python
  entrypoint: /usr/bin/python3.12 /app/app.py
  runtime layer: 5 shared libraries
  runtime layer: embedded /usr/lib/python3.12
  app layer: site-packages from .../bottle-web/site-packages
  runtime layer: 25.7MB -> 6.4MB (zstd, cached)
  app layer: 0.2MB -> 0.0MB (zstd)
[xbin] wrote ./bottle-web.xbin (7.1MB) in 25.1s
```

Rebuild après modification du code (couche runtime réutilisée) :

```
  runtime layer: reused from build cache (no recompression) ✓
  app layer: 0.2MB -> 0.0MB (zstd)
[xbin] wrote ./bottle-web.xbin (7.1MB) in 1.2s
```

## Limite connue (honnête)

En isolation **niveau 0**, l'ELF de l'interpréteur garde son interpreter path
codé en dur (`/lib64/ld-linux-x86-64.so.2`), résolu sur l'hôte. Le binaire tourne
donc parfaitement sur une machine **compatible** (même famille glibc), mais la
portabilité totale entre distributions exige le **niveau 2** (user namespaces +
`pivot_root`), où `/lib64/ld-linux` résout *dans* le rootfs. Voir
[Isolation](./isolation.md) et la [Roadmap](../roadmap.md).
