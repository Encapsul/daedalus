# Le launcher (stub)

Le launcher (« stub ») est le petit programme embarqué en tête du `.xbin`. C'est
l'ELF que le kernel exécute quand l'utilisateur lance `./mon_app.xbin`.

- **Code** : `stub/src/main.rs` + `stub/src/format.rs`
- **Langage** : Rust, compilé statiquement avec la cible
  `x86_64-unknown-linux-musl` → aucune dépendance dynamique, tourne partout.
- **Taille** : ~600 KB (objectif < 200 KB en optimisant les dépendances).

## Pourquoi Rust + musl ?

**Rust** : le launcher s'exécute avant toute vérification, manipule des fichiers
binaires et fait des appels système. Une erreur mémoire ici = faille. Rust
élimine par construction les use-after-free, buffer overflows et null
dereferences, sans runtime ni GC.

**musl** : `glibc` est liée dynamiquement et ses versions varient d'une distrib à
l'autre (`GLIBC_2.35 not found`…). Un binaire musl statique n'a **aucune**
dépendance dynamique — il tourne sur tout kernel Linux ≥ 3.8. C'est justement ce
que `xbin` doit garantir pour son propre launcher.

```bash
ldd stub/target/x86_64-unknown-linux-musl/release/xbin-stub
# → statically linked
```

## Le flux d'exécution

```
./mon_app.xbin
   │
   1. open("/proc/self/exe")          ← se localiser de façon fiable
   2. lire le footer (84 derniers bytes), valider magic
   3. lire les métadonnées JSON
   4. lire le payload, vérifier SHA-256       ← intégrité
   5. cache hit ? → ~/.cache/xbin/{sha256}/.ready
        oui → réutiliser
        non → extraire (zstd → tar) dans tmp, rename() atomique
   6. construire argv + env (injecter LD_LIBRARY_PATH)
   7. execve(entrypoint)              ← remplace le process
```

## Pourquoi `/proc/self/exe` et pas `argv[0]` ?

`argv[0]` est contrôlé par l'appelant : un process malveillant peut lancer le
launcher avec un `argv[0]` mensonger. `/proc/self/exe` est fourni par le kernel
et pointe **toujours** vers le vrai exécutable en cours. On lit donc toujours le
bon fichier.

## Décompression : pourquoi `ruzstd` et pas le crate `zstd` ?

Le crate `zstd` lie la bibliothèque C `libzstd`, ce qui nécessite un compilateur
C pour la cible musl. `ruzstd` est un décompresseur zstd **100 % Rust** : pas de
toolchain C, build musl statique trivial. Le launcher ne fait que **décompresser**
— c'est exactement le périmètre de `ruzstd`. La **compression** (qui demande plus
de CPU) reste côté builder, via le CLI `zstd`.

## Le token `${ROOTFS}` dans l'environnement

Le builder ne connaît pas à l'avance où le cache sera matérialisé
(`~/.cache/xbin/{sha256}/rootfs`). Pour qu'il puisse quand même déclarer des
chemins (ex : `PYTHONPATH`), il écrit le token `${ROOTFS}` dans les variables
d'environnement du manifest, et le launcher le remplace par le chemin réel au
moment de l'`exec` :

```
manifest :  PYTHONPATH = ${ROOTFS}/app/site-packages
exécution :  PYTHONPATH = /home/user/.cache/xbin/f342…/rootfs/app/site-packages
```

`LD_LIBRARY_PATH` n'a pas besoin de ce token : le launcher le calcule
directement à partir des répertoires `lib*` présents dans le rootfs.

## Accès concurrent : `flock()`

Si deux instances du même `.xbin` démarrent en même temps sur un cache froid, un
verrou exclusif `flock()` (sur `~/.cache/xbin/{hash}.lock`) garantit qu'une
seule fait l'extraction ; l'autre attend puis trouve le cache prêt. L'extraction
reste de toute façon atomique via `rename()` — le `flock` évite simplement le
travail dupliqué.

## Ce que le launcher fait à l'exécution (annoté)

Extrait de `main.rs` — la séquence est volontairement linéaire et lisible :

```rust
// 1. Se localiser de manière fiable (pas argv[0], contrôlable par l'appelant).
let mut exe = File::open("/proc/self/exe")?;
let footer = Footer::read_from(&mut exe)?;

// 2-3. Métadonnées puis payload, avec vérification d'intégrité.
let meta: Metadata = serde_json::from_slice(&meta_bytes)?;
verify_sha256(&payload, &footer.payload_sha256)?;

// 4. Cache : extraire une seule fois, atomiquement.
if !ready_marker.exists() {
    extract_atomic(&payload, &cache_root, &rootfs)?;
}

// 5. Remplacer le process courant par l'app.
exec_app(&meta, &rootfs)
```
