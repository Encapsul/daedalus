# Construire une app Python

## Structure attendue

`xbin` détecte une app Python à la présence d'un point d'entrée à la racine :
`app.py`, `main.py`, `server.py` ou `__main__.py`.

```
mon_app/
  app.py            ← point d'entrée détecté automatiquement
  ...               ← autres modules, templates, assets
```

## Construire

```bash
xbin build ./mon_app -o mon_app.xbin
```

Le builder :

1. détecte le runtime `python` et l'entrypoint (`/app/app.py`) ;
2. embarque l'interpréteur `python3` de la machine de build ;
3. embarque la **stdlib** (`/usr/lib/pythonX.Y`) ;
4. résout les `.so` via `ldd` (libc, etc.) ;
5. compresse et assemble le `.xbin`.

## Variables d'environnement

Le builder injecte par défaut `PYTHONUNBUFFERED=1` (logs en temps réel) et
`PYTHONDONTWRITEBYTECODE=1`. Ton app lit ses propres variables normalement :

```python
import os
PORT = int(os.environ.get("PORT", "8080"))
```

```bash
PORT=9000 ./mon_app.xbin
```

## Dépendances tierces (site-packages)

`xbin` embarque automatiquement les dépendances tierces. Il cherche, dans
l'ordre :

1. un virtualenv `.venv/` ou `venv/` à la racine de l'app → ses
   `lib/pythonX.Y/site-packages` ;
2. un dossier `site-packages/` vendu à la racine.

```
mon_app/
  app.py
  .venv/                ← détecté automatiquement, site-packages embarqués
    lib/python3.12/site-packages/
      bottle.py
```

Le builder :

- copie les site-packages sous `/app/site-packages` dans le rootfs ;
- déclare `PYTHONPATH=${ROOTFS}/app/site-packages` (le token `${ROOTFS}` est
  résolu par le launcher à l'exécution — voir [Launcher](../reference/launcher.md)) ;
- passe `ldd` sur les `.so` des extensions C (numpy, pillow…) pour embarquer
  leurs dépendances système.

L'exemple `examples/bottle-web` démontre ce cas : il sert du HTTP avec `bottle`,
un framework web qui n'est **pas** dans la stdlib.

```bash
xbin build ./examples/bottle-web -o bottle-web.xbin
./bottle-web.xbin   # → Hello from bottle, packagé par xbin
```

## Limites actuelles du MVP

- **`requirements.txt` sans venv** : le pip-install automatique au build (créer
  un venv temporaire et installer dedans) est la prochaine étape. Aujourd'hui,
  prépare un `.venv` (ou un dossier `site-packages/` vendu) à côté de ton app.
- **Portabilité inter-distribution** : voir [Isolation](../reference/isolation.md)
  — garantie complète au niveau 2 (user namespaces).
