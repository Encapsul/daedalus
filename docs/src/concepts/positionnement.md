# Positionnement

Le positionnement de `xbin` est sa décision la plus importante. Il définit ce
qu'on construit — et surtout ce qu'on ne construit pas.

## xbin cible le web/serveur headless, pas le desktop

| | Desktop GUI | Web / serveur headless |
|---|---|---|
| Exemples | VLC, Inkscape, GIMP | Next.js, FastAPI, outil de build CLI |
| Besoins | X11/Wayland, intégration bureau, icônes | un port réseau, stdout/stderr |
| Solution existante | **AppImage, Snap, Flatpak** | **rien de propre et language-agnostic** |

AppImage et consorts ont passé des années à régler l'intégration desktop. Ce
n'est ni le terrain ni l'ambition de `xbin`. Le créneau de `xbin` est celui que
ces outils ne visent pas : *« je lance le binaire, un serveur démarre, j'ouvre
mon navigateur »*.

## Pourquoi pas pkg / nexe / PyInstaller ?

Ces outils existent mais sont **mono-langage** :

- `vercel/pkg`, `nexe` → Node uniquement ;
- `PyInstaller`, `Nuitka`, `PyOxidizer` → Python uniquement ;
- `GraalVM native-image` → JVM uniquement.

`xbin` est **language-agnostic** par conception : il empaquette un *rootfs* (un
mini-filesystem), pas un langage en particulier. Le même outil packe une app
Python, une app Node, ou un binaire natif.

## Ce que xbin n'est pas

- **Pas un Docker killer.** Docker reste pour l'orchestration et le
  multi-container.
- **Pas une VM.** On ne virtualise pas le kernel ; l'app tourne sur le kernel
  hôte.
- **Pas un gestionnaire de paquets.** Pas de registry central (pour l'instant).

> Le positionnement juste : **« pour les cas où Docker est trop lourd et où
> AppImage ne s'applique pas, xbin est la réponse. »**

## Le cas qui a le plus de potentiel

La distribution de **modèles IA locaux** : empaqueter `llama.cpp` + un modèle de
plusieurs Go + un serveur d'inférence + une UI web, en un seul fichier qui se
lance avec `./mon_llm`. Aujourd'hui ce cas n'a pas de solution propre (Docker est
lourd, AppImage n'est pas pensé pour des payloads multi-Go, PyInstaller ne gère
pas les binaires C). L'architecture de `xbin` y est naturellement adaptée.
