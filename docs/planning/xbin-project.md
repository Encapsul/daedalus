# xbin — Documentation Technique du Projet

> *"Ship your app like a binary. Run anywhere."*

---

## 1. Le problème

Distribuer une application logicielle est inutilement compliqué en 2026.

Une app dépend d'un ensemble de choses installées sur la machine du développeur :

- Un **runtime** (Node.js, Python, Java, Ruby…)
- Des **librairies dynamiques système** (fichiers `.so` sur Linux)
- Des **packages** (node_modules, pip packages, gems…)
- Des **fichiers de configuration** et assets

Quand tu donnes cette application à quelqu'un d'autre — un collègue, un serveur de production, un client — **ça casse**. Soit Node n'est pas installé, soit c'est la mauvaise version, soit une librairie système manque, soit les paths sont différents.

C'est le problème historique du **"ça marche chez moi"**.

### Pourquoi Docker ne suffit pas

Docker a partiellement résolu ce problème mais introduit une friction considérable :

- Il faut **installer Docker** (daemon root, service système)
- Il faut **comprendre** les images, registries, volumes, networking
- Le daemon tourne **en permanence en root**
- C'est **beaucoup trop lourd** pour distribuer un simple outil CLI

**xbin résout ça différemment** : un seul fichier binaire, autonome, qui contient absolument tout ce dont l'app a besoin, et qui se lance comme un programme normal.

```bash
chmod +x mon_app && ./mon_app
# C'est tout. Zéro installation. Zéro configuration.
```

---

## 2. Ce qu'on construit

Un outil CLI appelé `xbin` avec quatre commandes :

```bash
xbin build  ./mon_app/          # analyse l'app et produit mon_app.xbin
xbin sign   mon_app.xbin        # signe le fichier avec une clé Ed25519
xbin run    mon_app.xbin        # lance le fichier
xbin inspect mon_app.xbin       # montre le contenu sans extraire
xbin clean  [--all]             # nettoie le cache local
```

Le fichier `.xbin` produit est un **exécutable autonome** qui contient :

1. Un **launcher ELF statique** — le petit programme qui démarre tout
2. Un **payload compressé zstd** — le rootfs complet (app + runtime + libs)
3. Des **métadonnées JSON** — entrypoint, architecture, version, timestamp
4. Une **signature Ed25519** — garantie d'intégrité et d'authenticité
5. Des **magic bytes + offsets** — pour que le launcher se localise lui-même

---

## 3. Secure by Design

### 3.1 Pourquoi la sécurité doit être dans l'architecture, pas ajoutée après

Un outil qui distribue et exécute du code arbitraire est une cible naturelle. Si xbin n'est pas sécurisé par conception, il devient un vecteur d'attaque parfait : un fichier `.xbin` malveillant pourrait exécuter n'importe quoi sur la machine de l'utilisateur.

Les quatre failles du design naïf et comment on les résout :

---

### 3.2 Faille 1 — Absence de vérification d'authenticité

**Problème naïf :** N'importe qui peut créer un `.xbin`. L'utilisateur n'a aucun moyen de savoir si le fichier vient d'une source fiable.

**Solution : Signature Ed25519 obligatoire**

Chaque `.xbin` doit être signé avant distribution. Le launcher vérifie la signature **avant d'extraire quoi que ce soit**. Si la signature est invalide ou absente, le fichier ne s'exécute pas.

```bash
# Côté développeur
xbin sign mon_app.xbin --key $XDG_DATA_HOME/xbin/mykey.pem

# Côté utilisateur (automatique, transparent)
./mon_app.xbin
# [xbin] verifying signature... OK (signed by ted@42.fr)
# [xbin] starting app...
```

Pourquoi Ed25519 et pas RSA ?
- **Ed25519** : clés de 32 bytes, signatures de 64 bytes, vérification rapide, résistant aux attaques par timing
- **RSA-2048** : clés de 256 bytes, signatures de 256 bytes, plus lent, vulnérable si mal implémenté
- Ed25519 est devenu le standard moderne (SSH, Signal, TLS 1.3)

---

### 3.3 Faille 2 — Race condition sur le cache (TOCTOU)

**Problème naïf :**

```
1. Check si ~/.cache/xbin/{hash}/ existe
2. [ici un attaquant peut injecter du contenu]
3. Utilise le contenu du cache
```

C'est une attaque **TOCTOU** (Time Of Check To Time Of Use). Entre le moment où on vérifie et le moment où on utilise, un attaquant peut substituer du contenu.

**Solution : Extraction atomique**

```
1. Extraire dans /tmp/xbin-{uuid-aléatoire}.tmp/   ← répertoire unique
2. Vérifier SHA-256 du contenu extrait
3. Vérifier la signature Ed25519
4. rename() atomique vers ~/.cache/xbin/{hash}/     ← atomique sur Linux
5. Si KO à n'importe quelle étape → rm -rf + exit
```

`rename()` est **atomique** sur les filesystems Linux. Soit le répertoire final existe et est valide, soit il n'existe pas. Pas d'état intermédiaire possible.

Rien n'est écrit en cache tant que la vérification complète n'est pas passée.

---

### 3.4 Faille 3 — Fallback non sécurisé LD_LIBRARY_PATH

**Problème naïf :** Le mode fallback `LD_LIBRARY_PATH` laisse l'app voir le filesystem host. Un attaquant peut placer une fausse librairie dans le répertoire courant qui sera chargée à la place de la vraie.

**Solution : Pas de fallback silencieux**

Si les user namespaces Linux ne sont pas disponibles, xbin refuse et explique pourquoi :

```bash
$ ./mon_app.xbin
[xbin] error: user namespaces not available on this system.
[xbin] xbin requires kernel >= 3.8 with unprivileged_userns_clone=1.
[xbin] check: sysctl kernel.unprivileged_userns_clone
[xbin] or contact your system administrator.
```

Mieux refuser proprement que tourner en mode non sécurisé silencieusement.

---

### 3.5 Faille 4 — Hash dans le fichier lui-même

**Problème naïf :** Si le SHA-256 est stocké dans le fichier `.xbin`, un attaquant qui modifie le payload peut mettre à jour le hash. Le SHA-256 seul ne protège que contre la corruption accidentelle.

**Solution : Signature cryptographique sur le hash**

```
SHA-256(payload + metadata) → Ed25519_sign(hash, private_key) → signature
```

La signature est vérifiée avec la clé publique de l'émetteur. Un attaquant qui modifie le payload invalide la signature — et il ne peut pas recréer une signature valide sans la clé privée.

---

### 3.6 La séquence d'exécution sécurisée complète

```
┌─────────────────────────────────────────────┐
│  ./mon_app.xbin                             │
│                                             │
│  1. Ouvre /proc/self/exe                   │
│  2. Lit le header, vérifie magic XBIN\x01  │
│  3. ── VÉRIFICATION SIGNATURE Ed25519 ──   │  ← RIEN ne se passe avant ça
│     Si invalide → exit(1), rien sur disque │
│  4. Vérifie SHA-256 du payload             │
│     Si invalide → exit(1), rien sur disque │
│  5. Extraction atomique dans /tmp/uuid.tmp/ │
│  6. rename() → ~/.cache/xbin/{hash}/       │
│  7. Crée user namespace Linux              │
│  8. pivot_root vers le rootfs              │
│  9. Applique filtre seccomp                │
│  10. exec entrypoint de l'app             │
└─────────────────────────────────────────────┘
```

**Règle fondamentale : rien n'est écrit sur le disque avant que la signature soit vérifiée.**

---

### 3.7 Trust model

Trois niveaux explicites :

| Niveau | Condition | Comportement |
|--------|-----------|--------------|
| `TRUSTED` | Signé par une clé dans `$XDG_DATA_HOME/xbin/trusted-keys/` | Exécution normale |
| `UNKNOWN` | Signé mais clé non dans le keyring | Warning + confirmation utilisateur |
| `UNSIGNED` | Non signé | Refus par défaut |

Pour les environnements de développement, on peut activer le mode permissif :

```bash
XBIN_ALLOW_UNSIGNED=1 ./mon_app.xbin
# [xbin] WARNING: running unsigned binary. Do not use in production.
```

---

## 4. Architecture technique

### 4.1 Le format binaire `.xbin`

Un fichier `.xbin` est structuré ainsi, dans l'ordre des bytes :

```
Offset 0-4    : Magic "XBIN\x01"           (5 bytes)  ← identifiant du format
Offset 5      : Version (u8)               (1 byte)   ← versioning du format
Offset 6      : Architecture (u8)          (1 byte)   ← 0x01=x86_64, 0x02=aarch64
Offset 7      : Flags (u8)                 (1 byte)   ← bit0=signé, bit1=chiffré
Offset 8-15   : Payload offset (u64 LE)    (8 bytes)  ← où commence le payload
Offset 16-23  : Payload size compressed    (8 bytes)  ← taille compressée
Offset 24-31  : Payload size uncompressed  (8 bytes)  ← taille originale
Offset 32-63  : SHA-256 du payload         (32 bytes) ← intégrité
Offset 64-71  : Metadata offset (u64 LE)   (8 bytes)  ← où est le JSON
Offset 72-79  : Metadata size (u64 LE)     (8 bytes)
Offset 80-143 : Signature Ed25519          (64 bytes) ← authenticité
Offset 144-175: Clé publique Ed25519       (32 bytes) ← pour vérification
Offset 176-179: Header CRC32               (4 bytes)  ← intégrité du header
[ELF code du launcher]
[payload zstd]
[metadata JSON]
[trailer: offset(u32) + magic 0xBEEFCAFE(u32)]
```

Le format commence par les magic bytes et le header. Le launcher ELF suit — il est techniquement un ELF valide que le kernel peut exécuter directement. Les données (payload + metadata) sont collées après le code ELF. Le trailer à la fin du fichier permet au launcher de se localiser en lisant `/proc/self/exe` depuis la fin.

---

### 4.2 Le principe de self-reading

Quand Linux exécute un fichier ELF, il charge le code en mémoire et démarre l'exécution. Le reste du fichier (payload + metadata + trailer) n'est pas chargé — il reste sur le disque.

Le launcher peut s'ouvrir lui-même via `/proc/self/exe` :

```rust
// Le launcher se lit lui-même
let exe = File::open("/proc/self/exe")?;
let size = exe.metadata()?.len();

// Lit le trailer (8 derniers bytes)
exe.seek(SeekFrom::End(-8))?;
let mut trailer = [0u8; 8];
exe.read_exact(&mut trailer)?;

// Vérifie le magic du trailer
let magic = u32::from_le_bytes(trailer[4..8].try_into()?);
assert_eq!(magic, 0xBEEFCAFE);

// Retrouve le début du payload
let payload_offset = u32::from_le_bytes(trailer[0..4].try_into()?);
```

Pourquoi `/proc/self/exe` et pas `argv[0]` ?

`argv[0]` est contrôlé par l'appelant. Un processus malveillant peut appeler xbin avec `argv[0]` pointant vers n'importe quel fichier. `/proc/self/exe` est fourni par le kernel et pointe **toujours** vers le vrai exécutable en cours.

---

### 4.3 Le cache

```
~/.cache/xbin/
  {sha256-du-payload}/
    rootfs/          ← filesystem extrait, prêt à l'emploi
    meta.json        ← timestamp d'extraction, version xbin utilisée
    .lock            ← flock() pour éviter double-extraction simultanée
    last_used        ← epoch timestamp, pour le LRU cleanup
```

**Extraction atomique :**

```
1. Générer /tmp/xbin-{uuid-v4}.tmp/
2. Extraire le payload zstd → tar → répertoire tmp
3. Vérifier SHA-256 de chaque fichier extrait
4. rename(/tmp/xbin-{uuid}.tmp/, ~/.cache/xbin/{hash}/)
   ↑ atomique sur Linux — pas d'état intermédiaire possible
```

**Accès concurrent :**

Si deux instances du même `.xbin` démarrent simultanément, `flock()` sur le fichier `.lock` garantit qu'une seule fait l'extraction. L'autre attend et trouve le cache déjà prêt.

**LRU cleanup :**

Si le cache dépasse 2GB, les entrées les moins récemment utilisées (champ `last_used`) sont supprimées automatiquement.

---

### 4.4 L'isolation via Linux namespaces

Linux propose 8 types de namespaces. xbin en utilise 3 :

**User namespace** (`CLONE_NEWUSER`) : permet à un utilisateur normal de se mapper en "root" à l'intérieur du namespace. C'est le préalable obligatoire qui donne les permissions pour créer les autres namespaces sans être root.

```
UID 1000 (host) → UID 0 (dans le namespace)
```

La magie : le kernel applique cette mapping. L'app croit être root. Le kernel sait que c'est l'UID 1000 qui agit — et refuse toutes les opérations privilégiées sur le host.

**Mount namespace** (`CLONE_NEWNS`) : espace de mounts isolé. Les bind mounts et pivot_root faits à l'intérieur n'affectent pas le host.

**PID namespace** (`CLONE_NEWPID`) : les PIDs sont réinitialisés à 1 à l'intérieur. L'app ne voit pas les processus du host.

**Séquence complète :**

```rust
// 1. Crée user + mount + PID namespaces (sans root)
unshare(CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_NEWPID)?;

// 2. Configure le mapping UID/GID
write("/proc/self/uid_map", "0 1000 1\n")?;   // root dans ns = uid 1000 dehors
write("/proc/self/setgroups", "deny")?;        // obligatoire avant gid_map
write("/proc/self/gid_map", "0 1000 1\n")?;

// 3. Bind-mount le rootfs sur lui-même (nécessaire pour pivot_root)
mount(rootfs, rootfs, MS_BIND | MS_REC)?;

// 4. pivot_root : rootfs devient la nouvelle racine
mkdir(rootfs + "/old_root")?;
pivot_root(rootfs, rootfs + "/old_root")?;
chdir("/")?;

// 5. Démonte l'ancien root (le host filesystem disparaît)
umount2("/old_root", MNT_DETACH)?;
rmdir("/old_root")?;

// 6. Applique seccomp
apply_seccomp_filter()?;

// 7. Exec l'app
execve(entrypoint, args, env)?;
```

Après `pivot_root` + `umount2`, **l'app ne peut physiquement pas accéder au filesystem host**. Pas de `/etc/passwd` host, pas de `/home`, pas de `/var`. Le kernel l'interdit.

---

### 4.5 Le filtre seccomp

seccomp (secure computing) installe un filtre BPF sur chaque syscall que le processus tente de faire. Si le syscall n'est pas dans la liste blanche, le processus est tué avec SIGSYS.

Le filtre est appliqué **après** `pivot_root` et **avant** `execve`. L'app démarre déjà filtrée.

Syscalls autorisés pour une app typique :

```
read, write, open, openat, close, stat, fstat, lstat
mmap, munmap, mprotect, brk
futex, nanosleep, clock_gettime
getpid, getuid, getgid
socket, connect, bind, listen, accept, send, recv
exit, exit_group
```

Syscalls toujours bloqués :

```
ptrace       ← ne peut pas débugger d'autres processus
mount        ← ne peut pas monter de filesystems
pivot_root   ← ne peut pas changer sa propre racine
reboot       ← ne peut pas éteindre le système
kexec_load   ← ne peut pas charger un autre kernel
```

---

## 5. La stack technique

### 5.1 Launcher stub — Rust + musl

**Pourquoi Rust ?**

Le launcher est le composant le plus critique. Il s'exécute avant toute vérification, il manipule des fichiers binaires, il fait des appels système bas niveau. Une erreur mémoire ici = faille de sécurité.

Rust élimine par construction :
- Les **use-after-free** (borrow checker)
- Les **buffer overflows** (bounds checking)
- Les **race conditions** (ownership model)
- Les **null pointer dereferences** (Option type)

Sans runtime, sans garbage collector, sans exceptions non gérées.

**Pourquoi musl et pas glibc ?**

glibc est liée dynamiquement sur presque tous les systèmes Linux. Mais les versions changent. Un binaire compilé sur Ubuntu 22.04 avec glibc 2.35 ne tourne pas sur Ubuntu 18.04 qui a glibc 2.27 — l'erreur `GLIBC_2.35 not found` est exactement ce que xbin doit résoudre.

musl est une libc alternative, légère, conçue pour être liée **statiquement**. Un binaire compilé avec musl n'a aucune dépendance dynamique. Il tourne partout où le kernel Linux le supporte (>= 3.8).

```bash
cargo build --target x86_64-unknown-linux-musl --release
ldd target/x86_64-unknown-linux-musl/release/xbin-stub
# → not a dynamic executable
```

**Crates utilisées :**

```toml
[dependencies]
ed25519-dalek  = "2"     # signature Ed25519, auditée
sha2           = "0.10"  # SHA-256
zstd           = "0.13"  # décompression zstd
nix            = "0.27"  # wrapper safe sur les syscalls Linux
seccompiler    = "0.4"   # filtres seccomp-bpf
serde_json     = "1"     # parsing metadata
```

**Taille cible :** < 200KB strippé. Obtenu via :

```toml
[profile.release]
opt-level     = "z"   # optimise pour la taille, pas la vitesse
lto           = true  # link-time optimization
codegen-units = 1     # un seul codegen unit = meilleure LTO
strip         = true  # supprime les symboles de debug
panic         = "abort" # pas de stack unwinding = plus petit
```

---

### 5.2 CLI et Builder — Python 3 + uv

**Pourquoi Python pour le builder ?**

Le builder est de la logique métier : parcourir des répertoires, appeler `ldd`, détecter des patterns dans des fichiers, appeler une API. Python est optimal pour ça :
- Bibliothèque standard riche (pathlib, subprocess, hashlib, json)
- Développement rapide
- Facile à modifier et tester
- Le SDK Claude (anthropic) est en Python

**Pourquoi uv et pas pip ?**

uv est un gestionnaire de paquets Python écrit en Rust, 10 à 100 fois plus rapide que pip. Installation des dépendances en secondes plutôt qu'en minutes. Crucial pour l'expérience développeur.

**Bibliothèques :**

```toml
[dependencies]
click       = "^8"     # CLI propre avec sous-commandes
rich        = "^13"    # output coloré et progress bars
anthropic   = "^0.25"  # SDK Claude pour l'analyse IA
pathlib2    = "*"      # manipulation de chemins
```

---

### 5.3 L'IA dans xbin — rôle précis

L'IA résout **un seul problème** mais il est réel : la détection des dépendances que `ldd` ne voit pas.

`ldd` analyse les dépendances déclarées à la compilation (dynamic linking). Mais beaucoup de dépendances sont découvertes à l'exécution :

```python
# ldd ne voit pas ça :
subprocess.run(['ffmpeg', '-i', input, output])   # binaire externe
ctypes.cdll.LoadLibrary('libcuda.so.1')           # dlopen dynamique
import importlib; importlib.import_module(plugin) # plugin dynamique
os.system('convert image.png output.jpg')         # ImageMagick
```

L'IA analyse le code source et détecte ces patterns :

```bash
xbin build ./mon_app/ --ai-analyze

[xbin AI] Analyzing codebase with Claude...
  Runtime: Python 3.11
  Framework: FastAPI
  External binaries detected:
    → ffmpeg (via subprocess.run)
    → convert (ImageMagick, via os.system)
  Dynamic libraries detected:
    → libcuda.so.1 (via ctypes, optional — GPU only)
  Environment variables required:
    → DATABASE_URL, SECRET_KEY, PORT

Generated xbin.manifest. Review before building.
```

C'est le seul endroit où l'IA apporte quelque chose qu'aucun outil statique ne peut faire : comprendre le **sens** du code, pas juste sa structure binaire.

---

## 6. Structure du projet

```
xbin/
├── stub/                        ← Rust — le launcher ELF
│   ├── src/
│   │   ├── main.rs              ← point d'entrée
│   │   ├── format.rs            ← parsing du format .xbin
│   │   ├── verify.rs            ← Ed25519 + SHA-256
│   │   ├── cache.rs             ← cache atomique + LRU
│   │   ├── namespace.rs         ← user ns + pivot_root
│   │   └── seccomp.rs           ← filtre syscalls
│   └── Cargo.toml
│
├── cli/                         ← Python — le builder et la CLI
│   ├── xbin/
│   │   ├── __init__.py
│   │   ├── cli.py               ← commandes click
│   │   ├── build.py             ← logique xbin build
│   │   ├── sign.py              ← logique xbin sign
│   │   ├── inspect.py           ← logique xbin inspect
│   │   ├── format.py            ← spec du format .xbin
│   │   ├── packer.py            ← assemblage du fichier final
│   │   └── analyzer/
│   │       ├── ldd.py           ← détection .so récursive
│   │       ├── runtime.py       ← détection Python/Node/bash/ELF
│   │       └── ai.py            ← analyse IA via Claude API
│   └── pyproject.toml
│
├── tests/
│   ├── test_format.py
│   ├── test_verify.py
│   ├── test_cache.py
│   └── fixtures/                ← .xbin de test
│
├── Makefile                     ← build stub + install CLI
└── README.md
```

---

## 7. Ordre d'implémentation

**Ne pas coder dans le désordre.** Chaque étape dépend de la précédente.

### Semaine 1 — Le format et la vérification

Commencer par `verify.rs` en Rust. C'est le composant le plus critique, il doit être parfait avant tout le reste.

- Spécifier le format `.xbin` sur papier (chaque field, chaque offset)
- Implémenter la génération de clés Ed25519 (`xbin keygen`)
- Implémenter la signature (`xbin sign`)
- Implémenter la vérification (`xbin verify`)
- Tests unitaires exhaustifs incluant les cas d'erreur

### Semaine 2 — Le launcher stub

- Parsing du header `.xbin`
- Self-reading via `/proc/self/exe`
- Cache atomique avec `flock()`
- Décompression zstd

### Semaine 3 — L'isolation

- User namespaces + uid/gid mapping
- pivot_root
- Filtre seccomp minimal
- Tests sur Ubuntu 18.04, 20.04, 22.04

### Semaine 4 — Le builder Python

- Détection des dépendances via `ldd` récursif
- Détection du runtime
- Construction du rootfs
- Assemblage du fichier `.xbin`
- CLI complète

### Semaine 5 — L'IA et la polish

- Intégration Claude API pour l'analyse de code
- Génération automatique du manifest
- Tests end-to-end sur des apps réelles
- Performance : warm start < 100ms

---

## 8. Métriques cibles

| Métrique | Cible |
|----------|-------|
| Warm start (cache hit) | < 100ms |
| Cold start (première extraction) | < 5s pour une app Python |
| Taille launcher stub strippé | < 200KB |
| App Python 50MB non compressée | → ~14MB compressée (ratio ~3.5x) |
| Overhead namespaces | < 20ms |

---

## 9. Ce que xbin n'est pas

- **Pas un Docker killer** — Docker reste pour l'orchestration, le multi-container, Kubernetes
- **Pas une VM** — pas de virtualisation kernel, l'app tourne sur le kernel host
- **Pas un gestionnaire de paquets** — pas de registry central, pas de versioning d'apps
- **Pas limité à un langage** — tout runtime embarquable fonctionne

Le positionnement juste : **"Pour les cas où Docker est trop lourd, xbin est la réponse."**

---

## 10. Comparaison avec les alternatives

| Critère | xbin | AppImage | Docker | Nix |
|---------|------|----------|--------|-----|
| Zéro installation | ✓ | ✓ | ✗ | ✗ |
| Détection auto deps | ✓ (+ IA) | ✗ (manuel) | ✗ | ✗ |
| Cache intelligent | ✓ | ✗ | ✓ | ✓ |
| Isolation sans root | ✓ | ✗ | ✗ | ✓ |
| Signature obligatoire | ✓ | optionnel | ✗ | ✓ |
| Warm start < 100ms | ✓ | ✓ | ✗ | ✗ |
| Format ouvert | ✓ | ✓ | ✗ | ✓ |
| IA pour les deps cachées | ✓ | ✗ | ✗ | ✗ |

---

*xbin — v0.1 — mai 2026*
