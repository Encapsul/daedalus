# Comparaison

## xbin vs les alternatives

| Critère | xbin | AppImage | Docker | pkg/PyInstaller |
|---|---|---|---|---|
| Cible | **web/serveur headless** | desktop GUI | tout (serveur) | un seul langage |
| Zéro installation côté user | ✅ | ✅ | ❌ (daemon) | ✅ |
| Language-agnostic | ✅ | ✅ | ✅ | ❌ |
| Un seul fichier | ✅ | ✅ | ❌ (image+runtime) | ✅ |
| Détection auto des deps | ✅ (+ IA visée) | ❌ (manuel) | ❌ | partielle |
| Cache intelligent | ✅ | partiel | ✅ | n/a |
| Isolation sans root | 🔜 (Phase 2) | ❌ | ⚠️ (rootless) | ❌ |
| Signature intégrée | 🔜 (Phase 2) | optionnel | ⚠️ | ❌ |
| Format ouvert | ✅ | ✅ | partiel | ❌ |

## Honnêteté sur la concurrence

Le marché se résout en partie **par en bas**, et il faut le savoir :

- **Go, Rust** produisent déjà un binaire statique unique — `xbin` n'y apporte
  rien.
- **Node 21+** intègre les *Single Executable Applications* (SEA) nativement.
- **Python** a PyInstaller, Nuitka, PyOxidizer, shiv, pex.
- **Java** a GraalVM native-image.

L'angle défendable de `xbin` n'est donc **pas** « faire un binaire unique » (déjà
résolu langage par langage), mais :

1. **language-agnostic** : un seul outil, même UX, quel que soit le runtime ;
2. **web/serveur headless** : un créneau qu'AppImage/Snap/Flatpak n'adressent
   pas ;
3. **détection des dépendances cachées par IA** : ce qu'aucun outil statique ne
   fait (subprocess, `dlopen`, plugins) — voir
   [Détection des dépendances](./guides/dependances.md) ;
4. **le cas modèles IA locaux** : payloads multi-Go + binaires natifs + serveur,
   sans solution propre aujourd'hui.

## La question à toujours pouvoir répondre

> *« Pourquoi pas AppImage / Docker / pkg ? »*

Réponse en une phrase : **AppImage c'est le desktop, Docker c'est lourd et il
faut un daemon, pkg/PyInstaller c'est mono-langage — xbin c'est le serveur web
headless, language-agnostic, en un fichier qui se lance tout seul.**
