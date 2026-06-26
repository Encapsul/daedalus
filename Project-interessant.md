Voilà le prompt complet :

---

**Prompt:**

Je veux construire un outil qui permet de transformer n'importe quelle application en un seul fichier exécutable autonome, qui peut tourner sur n'importe quelle machine Linux sans aucune installation préalable. L'utilisateur final doit juste faire `chmod +x mon_app && ./mon_app` et l'application démarre. Rien d'autre.

---

## Le problème qu'on résout

Aujourd'hui, distribuer une application est inutilement compliqué. Quand tu développes une app, elle dépend de plein de choses installées sur ta machine :

- Un runtime (Node.js, Python, Java, etc.)
- Des librairies dynamiques système (fichiers `.so` sur Linux)
- Des modules ou packages (node_modules, pip packages, etc.)
- Des fichiers de configuration ou assets

Quand tu donnes cette app à quelqu'un d'autre, ça casse. Soit Node n'est pas installé, soit c'est la mauvaise version, soit une lib système manque. C'est le problème historique du "ça marche chez moi".

Docker a partiellement résolu ce problème, mais il introduit énormément de friction : il faut installer Docker, un daemon tourne en root en permanence, il faut comprendre les images, les registries, les volumes, le networking Docker. C'est beaucoup trop lourd pour simplement distribuer une application.

Ce qu'on veut c'est l'opposé : un seul fichier binaire, autonome, qui contient tout ce dont l'application a besoin pour tourner, et qui se lance comme un programme normal.

---

## Ce qu'on veut construire concrètement

Un outil en ligne de commande qu'on appellera `xbin`, avec trois commandes principales :

```bash
xbin build ./mon_app    # produit un fichier mon_app.xbin
xbin run mon_app.xbin   # lance le fichier
xbin inspect mon_app.xbin # montre ce qu'il contient
```

Le fichier produit par `xbin build` est un exécutable autonome. Il contient :

1. Un launcher — un petit programme qui sait comment démarrer tout le reste
2. Toutes les dépendances de l'application compressées (libs, runtime, assets)
3. Des métadonnées (comment lancer l'app, quelle architecture, quel hash pour vérifier l'intégrité)

Le launcher est embarqué directement dans le fichier final. Il n'y a pas de runtime séparé à installer. Le fichier se suffit à lui-même.

---

## Comment ça marche techniquement

### Le format du fichier .xbin

On définit un format de fichier binaire structuré comme ceci :

```
[ELF launcher statique]     ← le programme qui démarre tout
[payload compressé zstd]    ← toutes les dépendances + l'app
[métadonnées JSON]          ← entrypoint, hash, architecture, etc.
[magic bytes + offsets]     ← pour que le launcher sache où tout est
```

Un fichier ELF (le format standard des exécutables Linux) peut avoir des données arbitraires collées après le code. Le launcher utilise cette technique : il lit sa propre fin de fichier pour trouver où commencent les données compressées, les extrait, et lance l'application dedans. C'est exactement le principe des archives auto-extractibles.

### Le builder

Le builder est la partie qui prépare le fichier .xbin. Il fait plusieurs choses :

**1. Analyse des dépendances**

Il détecte automatiquement tout ce dont l'application a besoin :
- Il utilise `ldd` pour trouver les librairies dynamiques (.so) dont dépend le binaire
- Il détecte le runtime nécessaire (est-ce une app Node ? Python ? Un binaire compilé ?)
- Il scanne les fichiers de l'application pour trouver les assets et fichiers de données

Il y a deux modes :
- Mode `auto` : détection automatique, best effort
- Mode `manifest` : l'utilisateur déclare explicitement les dépendances dans un fichier de config

**2. Construction du rootfs**

Il assemble un mini-filesystem (qu'on appelle rootfs, pour "root filesystem") qui contient exactement ce qu'il faut, rien de plus :

```
rootfs/
  lib/        ← les librairies .so nécessaires
  usr/lib/    ← le runtime (node, python...)
  app/        ← le code de l'application
  etc/        ← configuration minimale
```

Ce n'est pas un OS complet. C'est juste le strict minimum pour que l'app tourne.

**3. Compression et packaging**

Le rootfs est compressé avec zstd (algorithme de compression moderne, très rapide à décompresser). Puis le launcher ELF, le payload compressé et les métadonnées sont assemblés en un seul fichier.

### Le launcher

Le launcher est un petit programme écrit en Rust ou Go, compilé de manière statique avec musl (une alternative légère à glibc qui permet de produire des binaires vraiment portables, sans dépendance sur la libc de la machine hôte).

Quand l'utilisateur lance `./mon_app`, voici ce qui se passe :

1. Le launcher démarre (c'est lui le programme principal du fichier ELF)
2. Il calcule un hash du payload pour vérifier l'intégrité
3. Il vérifie si le contenu a déjà été extrait dans le cache (`~/.cache/xbin/{hash}/`)
4. Si non, il extrait le payload compressé dans ce dossier cache
5. Il crée un environnement isolé via chroot ou user namespaces Linux
6. Il lance l'application dans cet environnement
7. À la fin, il nettoie si nécessaire

### Le cache

Extraire les dépendances à chaque lancement serait lent. On utilise un cache intelligent :

```
~/.cache/xbin/
  {hash_du_fichier}/
    rootfs/       ← extrait une seule fois
    lock          ← évite la double extraction si deux instances tournent
    last_used     ← pour savoir quand nettoyer
```

La première exécution extrait tout. Les suivantes démarrent directement depuis le cache. L'objectif est un cold start inférieur à 100ms après la première extraction.

### L'isolation

On a trois niveaux d'isolation possibles, du plus simple au plus complexe :

**Niveau 0 — Pas d'isolation**
On extrait juste les fichiers et on lance le programme avec les bonnes variables d'environnement pour qu'il trouve ses libs (LD_LIBRARY_PATH). Simple, portable, mais le programme voit toute la machine.

**Niveau 1 — chroot**
On fait croire au programme que notre rootfs extrait est la racine du système. Il ne voit que ses propres libs. Problème : normalement chroot nécessite d'être root (administrateur).

**Niveau 2 — User namespaces**
Une feature du kernel Linux qui permet de faire un chroot et créer un environnement isolé SANS être root. C'est ce que Docker utilise en mode rootless. Plus complexe à implémenter mais ne demande aucun privilège à l'utilisateur.

Pour le MVP, on commence au niveau 0 ou 1, et on monte en isolation progressivement.

---

## L'architecture en couches

On décompose le projet en 4 couches avec des interfaces claires entre elles dès le départ, pour éviter la dette technique :

```
┌─────────────────────────────────┐
│           CLI (xbin)            │  interface utilisateur, commandes
├─────────────────────────────────┤
│           Builder               │  analyse les dépendances, construit le .xbin
│   ┌──────────┬──────────┐       │
│   │ Analyzer │ Packager │       │
│   └──────────┴──────────┘       │
├─────────────────────────────────┤
│           Runtime               │  extraction, cache, exécution
│   ┌──────────┬──────────┐       │
│   │  Cache   │ Executor │       │
│   └──────────┴──────────┘       │
├─────────────────────────────────┤
│        Format .xbin             │  spécification du fichier binaire
└─────────────────────────────────┘
```

Chaque couche a une responsabilité unique et ne connaît que la couche en dessous.

---

## Ce que ce projet n'est pas

- Ce n'est pas un "Docker killer". Docker reste pertinent pour l'orchestration, le networking complexe, les systèmes multi-containers.
- Ce n'est pas une VM. On ne virtualise pas le kernel.
- Ce n'est pas limité à un langage. L'objectif est de supporter n'importe quelle application : Node, Python, Rust, Go, Java, binaires C, etc.

---

## La stack technique envisagée

- **Langage principal** : Rust (contrôle système, binaire statique, performance) ou Go (prototypage rapide, cross-compilation simple)
- **Compression** : zstd
- **Format rootfs** : tar compressé pour le MVP, squashfs en optimisation
- **Launcher** : ELF statique compilé avec musl
- **Isolation** : LD_LIBRARY_PATH puis chroot puis user namespaces, progressivement
- **Cache** : filesystem local dans ~/.cache/xbin/

---

## Les défis techniques principaux

**1. La portabilité de glibc**
La plupart des programmes Linux sont compilés contre glibc, qui n'est pas la même version partout. Le launcher lui-même doit être compilé avec musl pour être vraiment portable. Pour les dépendances embarquées, on doit embarquer la bonne version de glibc ou patcher les binaires pour utiliser celle qu'on embarque.

**2. La détection automatique des dépendances**
`ldd` trouve les libs dynamiques, mais certaines dépendances sont chargées dynamiquement à l'exécution (avec `dlopen`) et ne sont pas détectables statiquement. Il faut une stratégie pour ces cas.

**3. Le cold start**
L'extraction complète peut prendre du temps pour de grosses applications. Le cache résout le problème après la première fois, mais la première exécution doit rester acceptable.

**4. Les applications avec accès GPU, /dev, sockets**
Certaines apps ont besoin d'accéder à du matériel spécifique. C'est hors scope du MVP mais à garder en tête dans la conception.

---

## La roadmap en 3 phases

**Phase 1 — MVP fonctionnel**
- Format .xbin défini et stable
- Builder qui détecte les dépendances via ldd et embarque le runtime
- Launcher qui extrait et lance avec LD_LIBRARY_PATH
- Support Node.js et Python d'abord
- CLI basique : build, run, inspect

**Phase 2 — Robustesse**
- Cache intelligent avec hash
- Isolation via chroot sans root (user namespaces)
- Mode manifest pour les dépendances complexes
- Cold start optimisé

**Phase 3 — Produit fini**
- squashfs + mmap (pas d'extraction, lecture directe)
- Cold start < 100ms
- Support de tous les langages et runtimes
- Signature et vérification d'intégrité
- Distribution peer-to-peer

---

Je veux qu'on commence par la conception architecturale détaillée avant d'écrire une seule ligne de code. Aide-moi à prendre les bonnes décisions structurantes sur le format du fichier, les interfaces entre les couches, et la stratégie de détection des dépendances. L'objectif est d'avoir une base solide qui ne nécessitera pas de tout réécrire quand on passera d'une phase à l'autre.
