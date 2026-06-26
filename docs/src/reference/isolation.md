# Isolation

`xbin` prévoit trois niveaux d'isolation, du plus simple au plus complet. Le MVP
implémente le niveau 0 ; les niveaux 1 et 2 sont la Phase 2.

## Niveau 0 — `LD_LIBRARY_PATH` (MVP actuel)

On extrait le rootfs et on lance l'app avec les bonnes variables d'environnement
pour qu'elle trouve ses libs :

```
LD_LIBRARY_PATH = rootfs/lib:rootfs/lib64:rootfs/usr/lib:...
```

- ✅ Simple, portable, aucun privilège requis.
- ✅ Suffit quand la machine cible est compatible (même famille glibc).
- ⚠️ L'app voit tout le filesystem de l'hôte (aucune isolation).
- ⚠️ L'interpreter path de l'ELF (`/lib64/ld-linux…`) reste résolu sur l'hôte →
  portabilité inter-distribution non garantie.

## Niveau 1 — chroot

On fait croire à l'app que le rootfs extrait *est* la racine du système. Elle ne
voit que ses propres libs. Limite : `chroot()` requiert classiquement d'être
root, ce qu'on ne veut **pas** demander.

## Niveau 2 — User namespaces (cible)

La fonctionnalité du kernel Linux qui permet de faire un chroot et d'isoler
l'environnement **sans être root**. C'est ce qu'utilise Docker en mode rootless.

```rust
// 1. Créer user + mount + PID namespaces (sans root)
unshare(CLONE_NEWUSER | CLONE_NEWNS | CLONE_NEWPID)?;

// 2. Mapper l'UID courant vers "root" dans le namespace
write("/proc/self/uid_map", "0 1000 1")?;
write("/proc/self/setgroups", "deny")?;
write("/proc/self/gid_map", "0 1000 1")?;

// 3. pivot_root vers le rootfs, puis démonter l'ancienne racine
pivot_root(rootfs, rootfs + "/old_root")?;
umount2("/old_root", MNT_DETACH)?;

// 4. (optionnel) filtre seccomp, puis exec
execve(entrypoint, argv, env)?;
```

Avantage décisif : après `pivot_root` + `umount2`, l'app ne peut
**physiquement** plus accéder au filesystem hôte, et `/lib64/ld-linux` résout
**dans** le rootfs → portabilité réelle entre distributions.

## Décision de conception

> L'isolation est une **feature**, pas le cœur du produit. La proposition de
> valeur de `xbin`, c'est *« un fichier, ça tourne »*. On démarre donc au niveau
> 0 (qui prouve le pipeline complet) et on monte en isolation **sans changer le
> format `.xbin`** : seul le champ `isolation` des métadonnées et le
> comportement du launcher évoluent.

## Stratégie de fallback (à trancher)

Quand les user namespaces ne sont pas disponibles (certains serveurs corporate
les désactivent : `kernel.unprivileged_userns_clone=0`), deux écoles :

- **fail-safe (sécurité d'abord)** : refuser et expliquer pourquoi ;
- **fallback (UX d'abord)** : retomber sur le niveau 0 avec un avertissement.

Le choix dépend du marché visé (entreprise vs développeur). C'est documenté comme
une décision ouverte — voir [Sécurité](../securite.md).
