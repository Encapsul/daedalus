# Sécurité

Un outil qui distribue et exécute du code est une cible naturelle. La sécurité de
`xbin` doit être **dans l'architecture**, pas ajoutée après. Voici les failles du
design naïf et leur parade. (Les signatures sont Phase 2 ; l'intégrité SHA-256 et
l'extraction atomique sont **déjà** dans le MVP.)

## 1. Authenticité — signature Ed25519 (Phase 2)

**Problème** : n'importe qui peut fabriquer un `.xbin`. L'utilisateur n'a aucun
moyen de savoir d'où il vient.

**Parade** : chaque `.xbin` est signé. Le launcher vérifie la signature **avant
d'extraire quoi que ce soit**. Signature invalide ou absente → refus.

Pourquoi Ed25519 et pas RSA : clés/sig courtes (32/64 bytes), vérification rapide,
résistant aux attaques par timing. C'est le standard moderne (SSH, TLS 1.3,
Signal).

## 2. Race condition sur le cache — TOCTOU ✅ (déjà géré)

**Problème** : entre la vérification de l'existence du cache et son utilisation,
un attaquant pourrait substituer du contenu.

**Parade** : extraction dans un répertoire temp unique, puis `rename()`
**atomique** vers le cache final. Aucun état intermédiaire exposé. Voir
[Cache](./reference/cache.md).

## 3. Intégrité — SHA-256 ✅ (déjà géré)

Le launcher recalcule le SHA-256 du payload et le compare à celui du footer
**avant** extraction. En cas de divergence : `exit(1)`, rien n'est écrit sur
disque.

> Le SHA-256 seul protège contre la **corruption**, pas contre un attaquant qui
> recalculerait le hash après modification. C'est précisément pourquoi la Phase 2
> **signe** le hash : `Ed25519_sign(SHA256(payload+meta), clé_privée)`. Sans la
> clé privée, impossible de forger une signature valide.

## 4. Fallback non sécurisé `LD_LIBRARY_PATH`

**Problème** : le mode niveau 0 laisse l'app voir le filesystem hôte ; une fausse
lib placée dans un répertoire visible pourrait être chargée.

**Parade cible** : au niveau 2 (user namespaces + `pivot_root`), l'app ne voit
**que** son rootfs. Question ouverte : si les user namespaces sont indisponibles,
faut-il refuser (sécurité d'abord) ou retomber au niveau 0 avec un avertissement
(UX d'abord) ? Le choix dépend du marché — voir [Isolation](./reference/isolation.md).

## La séquence sécurisée cible

```
1. open(/proc/self/exe)
2. lire le footer, valider magic
3. ── VÉRIFIER LA SIGNATURE Ed25519 ──   ← rien ne se passe avant (Phase 2)
4. vérifier le SHA-256 du payload         ← déjà dans le MVP
5. extraction atomique (tmp → rename)     ← déjà dans le MVP
6. user namespace + pivot_root            ← Phase 2
7. filtre seccomp                          ← Phase 2
8. exec entrypoint
```

> **Règle fondamentale** : rien n'est écrit sur disque avant que l'intégrité
> (MVP) — et bientôt la signature (Phase 2) — soit vérifiée.
