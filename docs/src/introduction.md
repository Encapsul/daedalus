# x.bin

> *Ship your web app like a binary. Run anywhere.*

**x.bin** — le `x` pour *n'importe quelle app*, le `.bin` pour *un binaire*.
La commande en ligne de commande s'appelle `xbin` et les fichiers produits
portent l'extension `.xbin` (un point ne peut pas figurer dans un nom de
commande shell).

`x.bin` transforme n'importe quelle application **web / serveur / outil headless**
en **un seul fichier exécutable autonome**. L'utilisateur final n'installe rien :

```bash
chmod +x mon_app.xbin && ./mon_app.xbin
# [xbin] starting app...
# Server listening on http://127.0.0.1:8080
```

Pas de runtime à installer. Pas de `node` ni `python` sur la machine cible. Pas
de Docker. Un fichier, on le rend exécutable, on le lance.

## En une phrase

`xbin` est à une app serveur ce qu'un binaire Go statique est à un programme
compilé : **tout est dedans, ça tourne partout.**

## Ce qui est différent

AppImage, Snap et Flatpak résolvent ce problème pour les applications **desktop
GUI** (elles ont besoin de X11, d'intégration au bureau, d'icônes…). `xbin` vise
l'angle opposé et largement ignoré : les **apps web et serveur headless** — un
serveur Next.js, une API FastAPI, un outil de build CLI. Tu lances le binaire,
le serveur démarre, tu ouvres ton navigateur.

Et contrairement à `vercel/pkg` ou `nexe` (Node uniquement), `xbin` est
**language-agnostic** : Python, Node, Go, binaires natifs.

## État du projet

MVP fonctionnel — Phase 1. Le pipeline complet tourne de bout en bout :
`build` → `.xbin` → exécution avec auto-extraction et cache. Voir la
[Roadmap](./roadmap.md) pour la suite (signature Ed25519, user namespaces,
squashfs+mmap).
