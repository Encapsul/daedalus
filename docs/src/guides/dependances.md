# Détection des dépendances

C'est là que vit ~70 % de la complexité — et la différenciation de `xbin`.

## Ce que `ldd` voit

`ldd` liste les librairies dynamiques déclarées à la compilation. Le builder
l'utilise (`analyzer/ldd.py`) et récupère, transitivement, toutes les `.so` plus
le dynamic loader. Pour un binaire compilé classique, c'est suffisant.

```
ton binaire ─ldd→ libc.so.6, libssl.so.3, … , ld-linux-x86-64.so.2
```

## Ce que `ldd` ne voit PAS

Beaucoup de dépendances ne sont découvertes qu'à **l'exécution** :

```python
subprocess.run(["ffmpeg", "-i", src, dst])      # binaire externe
ctypes.cdll.LoadLibrary("libcuda.so.1")           # dlopen dynamique
importlib.import_module(plugin_name)               # plugin chargé par nom
os.system("convert in.png out.jpg")                # ImageMagick
```

Aucun outil **statique** ne peut les trouver de façon fiable : il faut
*comprendre* le code, pas juste lire sa table de symboles.

## Deux modes

- **`auto`** (défaut) : détection best-effort via `ldd` + heuristiques runtime.
- **`manifest`** : l'utilisateur déclare explicitement dans un `xbin.toml` les
  binaires externes, libs `dlopen`, variables d'env requises, fichiers de
  données. C'est le filet de sécurité quand l'auto ne suffit pas.

```toml
# xbin.toml (cible)
[deps]
binaries = ["ffmpeg", "convert"]
libraries = ["libcuda.so.1"]   # optionnel, GPU
[env]
required = ["DATABASE_URL", "SECRET_KEY", "PORT"]
```

## Le rôle de l'IA (différenciation)

L'IA résout **un seul** problème, mais réel : détecter ces dépendances cachées.
Elle analyse le code source et repère les `subprocess`, `dlopen`, plugins
dynamiques, variables d'environnement requises, puis **génère un `xbin.toml`** que
l'utilisateur relit avant le build.

```
xbin build ./mon_app --ai-analyze
[xbin AI] Runtime: Python 3.11 / FastAPI
  External binaries: ffmpeg (subprocess), convert (os.system)
  Dynamic libs:      libcuda.so.1 (ctypes, optionnel)
  Env required:      DATABASE_URL, SECRET_KEY, PORT
Generated xbin.manifest. Review before building.
```

> C'est le seul endroit où l'IA apporte ce qu'aucun outil statique ne peut faire.
> Statut : **Phase 2** — l'interface (`--ai-analyze` → `xbin.toml`) est conçue
> pour que l'IA soit un *générateur de manifest*, jamais un point de passage
> obligatoire opaque.
