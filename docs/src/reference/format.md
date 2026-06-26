# Format `.xbin`

Un fichier `.xbin` est un **ELF exécutable valide** auquel on a concaténé un
payload, des métadonnées, et un **footer** à la toute fin du fichier.

Le format est **versionné** (`format_version` dans le footer). La v1 utilise un
payload monolithique ; la **v2** (actuelle) découpe le payload en **couches**
(voir plus bas). Le launcher lit les deux.

## Pourquoi un footer à la fin, et pas un header au début ?

C'est le point de conception le plus important du format.

Le kernel Linux exige que le magic ELF (`\x7fELF`) soit à **l'offset 0** pour
exécuter le fichier. On ne peut donc **pas** mettre nos propres magic bytes au
début sans casser l'exécutabilité.

La solution (celle de `makeself`, AppImage, des `.exe` auto-extractibles) :

- l'**ELF du launcher** occupe le début du fichier — le kernel l'exécute ;
- nos données sont **collées après** ;
- un **footer de taille fixe** est placé en toute fin de fichier ;
- au démarrage, le launcher s'ouvre via `/proc/self/exe`, fait un `seek` depuis
  la fin, lit le footer, et y trouve les offsets de tout le reste.

```
offset 0    ┌─────────────────────────┐
            │   ELF launcher (musl)   │  ← exécuté par le kernel
            ├─────────────────────────┤
payload_off │   payload = zstd(tar)   │  ← rootfs compressé
            ├─────────────────────────┤
meta_off    │   metadata (JSON utf8)  │  ← entrypoint, env, runtime…
            ├─────────────────────────┤
EOF - 84    │   FOOTER (84 bytes)     │  ← lu à l'envers par le launcher
            └─────────────────────────┘
```

## Le footer (84 bytes fixes)

Tous les entiers sont en little-endian.

| champ            | type  | taille | description                              |
|------------------|-------|--------|------------------------------------------|
| `magic`          | bytes | 5      | `"XBIN\x01"`                             |
| `format_version` | u8    | 1      | version du format (= 1)                  |
| `arch`           | u8    | 1      | `0x01`=x86_64, `0x02`=aarch64            |
| `flags`          | u8    | 1      | bit0=signé, bit1=chiffré (0 en MVP)      |
| `payload_offset` | u64   | 8      | offset absolu du payload                 |
| `payload_csize`  | u64   | 8      | taille compressée                        |
| `payload_usize`  | u64   | 8      | taille décompressée (tar)                |
| `payload_sha256` | bytes | 32     | SHA-256 du payload compressé             |
| `meta_offset`    | u64   | 8      | offset absolu des métadonnées            |
| `meta_size`      | u64   | 8      | taille des métadonnées                   |
| `footer_magic`   | u32   | 4      | `0xBEEFCAFE` — sentinelle de fin         |

Total : **84 bytes**. La spec est partagée entre `stub/src/format.rs` (lecture)
et `cli/xbin/format.py` (écriture) — les deux **doivent** rester synchronisés.

## Métadonnées JSON

```json
{
  "name": "hello-web",
  "xbin_version": "0.1.0",
  "created": "2026-06-23T12:00:00Z",
  "runtime": "python",
  "isolation": 0,
  "entrypoint": ["/usr/bin/python3.12", "/app/app.py"],
  "env": { "PYTHONUNBUFFERED": "1" },
  "cwd": "/app"
}
```

- `entrypoint` : argv exécuté par le launcher. Les chemins absolus sont
  **relatifs au rootfs** (le launcher les préfixe avec le chemin réel du cache).
- `env` : variables ajoutées (le launcher injecte `LD_LIBRARY_PATH` en plus).
- `cwd` : working directory dans le rootfs.
- `isolation` : 0 = `LD_LIBRARY_PATH`, 1 = chroot, 2 = user namespaces.

## Les couches (v2)

En v2, le payload n'est pas un bloc unique mais une suite de **couches**, chacune
un blob `zstd(tar)` indépendant, empilées à l'extraction (les couches suivantes
écrasent les précédentes — modèle proche des layers Docker) :

```
[ stub ][ couche runtime ][ couche app ][ metadata ][ footer ]
         ^                  ^
         python+stdlib+.so  code de l'app + site-packages
         stable             volatil
```

Le **footer** garde la même structure de 84 bytes ; sa sémantique s'adapte :

| champ            | v1                    | v2                                  |
|------------------|-----------------------|-------------------------------------|
| `payload_offset` | début du payload      | début de la **région des couches**  |
| `payload_csize`  | taille du payload     | taille totale des couches           |
| `payload_usize`  | taille décompressée   | inutilisé (tailles par couche)      |
| `payload_sha256` | SHA-256(payload)      | SHA-256(**couches ‖ metadata**)     |

La **table des couches** vit dans les métadonnées JSON :

```json
"layers": [
  {"kind": "runtime", "offset": 614960, "csize": 6710886, "usize": 26953728,
   "sha256": "168a7279b815…"},
  {"kind": "app",     "offset": 7325846, "csize": 12044,   "usize": 204800,
   "sha256": "9e61cd65ed9e…"}
]
```

`offset` est l'offset **absolu** dans le fichier. Le `sha256` de chaque couche
(du blob compressé) sert de **clé de cache stable** : tant que le contenu d'une
couche ne change pas, son extraction est réutilisable.

### Pourquoi des couches : le rebuild incrémental

C'est la raison d'être de la v2. La couche **runtime** (interpréteur + stdlib +
`.so`) est **indépendante du code de l'app** : éditer `app.py` ne la change pas.
Au rebuild, le builder la **réutilise telle quelle** depuis son cache de build
(`~/.cache/xbin/build/`) — pas de recompression. Seule la petite couche **app**
est refaite.

```
build initial  : ~25 s  (compression de la couche runtime, ~26 MB)
rebuild (code)  : ~1 s   (couche runtime réutilisée, seule l'app recompresse)
```

Bonus : deux apps qui partagent le même runtime (même interpréteur + libs)
partagent la **même couche runtime** dans le cache de build — la seconde app se
build en ~1 s elle aussi. Voir [Le builder](./builder.md).

## Pourquoi le format survit aux évolutions

- Le footer est **versionné** (`format_version`). Un launcher refuse proprement
  un fichier de version supérieure à ce qu'il sait lire.
- Les champs réservés (`flags`, et l'ajout d'un bloc signature en Phase 2)
  permettent d'étendre sans casser la compatibilité.
- La signature Ed25519 s'insérera entre `metadata` et `footer`, avec un bit
  `flags` et un offset dédié dans un footer v2 — les fichiers v1 restent
  lisibles.
