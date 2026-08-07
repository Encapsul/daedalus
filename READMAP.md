# READMAP — portabilité du stub et items à gérer

Ongoing issues that block or constrain a fully multi-platform release.
Each entry lists the symptom, the current state, and what is needed to close it.

## 1. Le stub ne compile/ne tourne pas sur riscv64 et ppc64le

**Symptôme** : `xbin-stub` échoue à compiler sur `riscv64gc-unknown-linux-musl`
et `powerpc64le-unknown-linux-musl` avec 21 erreurs `E0425` (constantes
seccomp manquantes : `AUDIT_ARCH`, `LD_PATHS`, `SYS_PTRACE`, `SYS_MOUNT`,
`SYS_PIVOT_ROOT`, `SYS_KEXEC_*`, `SYS_INIT_MODULE`, …).

**Cause** : `stub/src/main.rs` ne définit les constantes du filtre seccomp
que pour `x86_64`, `aarch64`, `x86` (i686) et `arm`. Le filtre est
security-critical (syscalls ptrace/mount/pivot_root/…) et les numéros de
syscalls riscv64 (asm-generic) et ppc64le (arch/powerpc) diffèrent ; ils ne
sont pas encore câblés, faute de pouvoir les vérifier.

**Décision actuelle** : riscv64 et ppc64le sont publiés **CLI-only** dans le
pipeline de release — le stub n'y est ni construit ni empaqueté. Le sandbox
seccomp reste complet sur les 4 archs supportés.

**Pour fermer** (options, à trancher) :
1. Câbler les constantes riscv64/ppc64le en les vérifiant depuis les headers
   kernel (`asm-generic/unistd.h`, `arch/powerpc/include/uapi/asm/unistd.h`)
   et valider le filtre au runtime sur vraie machine/QEMU.
2. Compile-time gate : désactiver seccomp sur les archs non supportés en
   gardant landlock + namespaces (dégradation assumée du poste de sécurité).
3. Laisser ces targets en CLI-only définitivement.

## 2. s390x : pas de build possible

`s390x-unknown-linux-musl` : pas de prébuild `rustup` (aucune libstd) et le
CLI échoue aussi (5 erreurs, dépendances sans support s390x). Target à
exclure ou à retenter quand un prébuild existe.

## 3. Windows

Le CLI compile pour `x86_64-pc-windows-gnu` et `i686-pc-windows-gnu`
(`xbin.exe`, tar.rs porté). Le stub est Linux-only (unshare/pivot_root/
seccomp/landlock) — pas de port Windows prévu. Les archives Windows ne
contiennent que `xbin.exe`.

## 4. BSD (FreeBSD/NetBSD/OpenBSD)

Cross-compilation via zigbuild impossible (link fail : libs systèmes manquantes
via zig ; pas de std prébuildé rustup pour NetBSD/OpenBSD). Nécessite des
VMs natives dans le CI (`vmactions/freebsd-vm`, `netbsd-vm`, `openbsd-vm`).
À ajouter au release.yml.

## Suivi

Chaque item ci-dessus doit être fermé par une PR (fmt + clippy + tests)
avant de considérer la matrice de release complète.
