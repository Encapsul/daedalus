# Le cache

Extraire le rootfs à chaque lancement serait lent. `xbin` extrait **une seule
fois** puis réutilise.

## Layout

```
~/.cache/xbin/
  {sha256-du-payload}/
    rootfs/      ← filesystem extrait, prêt à l'emploi
    .ready       ← marqueur : l'extraction est complète et valide
```

La clé du cache est le **SHA-256 du payload compressé**. Deux `.xbin` au contenu
identique partagent le même cache ; un changement d'un seul byte produit une
nouvelle clé. (`.lock` pour l'accès concurrent et `last_used` pour le nettoyage
LRU arrivent — voir [Roadmap](../roadmap.md).)

## Extraction atomique (anti-TOCTOU)

Le danger : entre le moment où on vérifie que le cache existe et le moment où on
l'utilise, un attaquant pourrait y injecter du contenu (attaque *Time Of Check To
Time Of Use*). La parade est de ne **jamais** exposer un état intermédiaire :

```
1. extraire dans  ~/.cache/xbin/.tmp-{pid}-{nanos}/   ← répertoire unique
2. écrire .ready une fois l'extraction terminée
3. rename() vers ~/.cache/xbin/{sha256}/              ← atomique sur Linux
4. si une autre instance a gagné la course → jeter notre tmp
```

`rename()` est **atomique** sur un même filesystem : soit le répertoire final
existe et est complet, soit il n'existe pas. Pas d'état à moitié écrit. C'est
pour ça que le tmp est créé dans le **même** dossier parent que la cible.

## Cold start vs warm start

| | Première exécution | Exécutions suivantes |
|---|---|---|
| Cache | absent → extraction | présent (`.ready`) → réutilisé |
| Message | `cold start: extracting…` | `warm start: cache hit {hash}` |
| Coût | décompression zstd + écriture disque | quasi nul côté xbin |

> Aujourd'hui le « temps jusqu'au premier byte HTTP » en warm est dominé par le
> **boot du runtime embarqué** (démarrage de l'interpréteur Python, imports), pas
> par `xbin`. L'overhead propre du launcher est de l'ordre de la milliseconde.
> L'objectif < 100 ms de bout en bout passera par squashfs+mmap (pas
> d'extraction) en Phase 3.

## Deux caches distincts

| | Cache d'extraction | Cache de build |
|---|---|---|
| Chemin | `~/.cache/xbin/{sha256}/` | `~/.cache/xbin/build/{hash}.zst` |
| Côté | machine **cible** (au `run`) | machine **de build** (au `build`) |
| Contenu | rootfs extrait, prêt à l'emploi | couches **compressées** réutilisables |
| Rôle | éviter de ré-extraire à chaque lancement | éviter de recompresser au rebuild |

Le cache de build est ce qui fait passer un rebuild de ~25 s à ~1 s (couche
runtime réutilisée). Voir [Format .xbin](./format.md#les-couches-v2).

## Clé de cache d'extraction en v2

En v2, la clé n'est plus le hash du payload mais le **SHA-256 de la concaténation
des hash de couches**. Tant que les couches sont identiques, l'entrée extraite
est réutilisée. (La réutilisation *par couche* — extraire la couche runtime une
seule fois et superposer des couches app via overlayfs — viendra avec
l'isolation niveau 2 ; aujourd'hui un changement de couche app ré-extrait
l'ensemble côté cible, mais le gain au **build** est déjà acquis.)

## Nettoyage

```bash
xbin clean        # vide les entrées extraites, CONSERVE le cache de build
xbin clean --all  # vide tout ~/.cache/xbin (build cache inclus)
```
