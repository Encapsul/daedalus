# Isolation

`daedalus` offers three isolation levels. Levels 0 and 2 are available;
level 1 is skipped because level 2 makes it redundant.

## Level 0 — `LD_LIBRARY_PATH` (available)

Extract the rootfs and launch the app with the right environment variables
to find its libs:

```
LD_LIBRARY_PATH = rootfs/lib:rootfs/lib64:rootfs/usr/lib:...
```

- ✅ Simple, portable, no privileges required.
- ✅ Sufficient when the target machine is compatible (same glibc family).
- ⚠️ The app sees the entire host filesystem (no isolation).
- ⚠️ The ELF interpreter path (`/lib64/ld-linux...`) still resolves on the
  host → cross-distro portability not guaranteed.

## Level 2 — User namespaces ✅ (implemented, recommended)

A Linux kernel feature that allows chroot-like isolation **without root
privileges**. This is what Docker uses in rootless mode.

Mechanism (implemented in `stub/src/main.rs`):

1. `unshare(CLONE_NEWUSER | CLONE_NEWNS)` — new user + mount namespaces
2. UID/GID mapping: write to `/proc/self/uid_map`, `setgroups`, `gid_map`
3. Bind-mount the rootfs onto itself (`MS_BIND | MS_REC`, required by
   `pivot_root`)
4. `pivot_root(rootfs, rootfs/.old_root)` + `umount2("/.old_root", MNT_DETACH)`
5. `std::env::set_current_dir("/")` — CWD in the new root
6. Install seccomp-bpf denylist (blocks ~14 dangerous syscalls)
7. `execvp(entrypoint)` — the app sees only its rootfs

Usage:
```
daedalus build my-app/ -o my-app.daedalus --isolation 2
```

**Implementation details:**
- The builder preserves symbolic links (e.g. `/lib64/ld-linux-x86-64.so.2`)
  in the rootfs so `pivot_root` resolves them correctly.
- The ELF analyzer deduplicates `.so` files that point to the same real file
  via different symlinks, avoiding duplicates in the rootfs.
- `/etc/hosts` is created in the rootfs (`127.0.0.1 localhost`) to prevent
  the DNS PTR lookup hang that blocked app startup.
- The seccomp filter is a conservative denylist — only syscalls with no
  legitimate use in web/server apps are blocked (ptrace, mount, reboot,
  kexec, module loading, etc.). All networking, file I/O, memory, and
  process syscalls pass through.
- If seccomp is unavailable (kernel without `CONFIG_SECCOMP`), the launcher
  prints a warning and continues without the filter.

## Level 1 — chroot (skipped)

Make the app think the extracted rootfs *is* the system root. It only sees
its own libs. Limitation: `chroot()` traditionally requires root, which we
don't want to ask for → skip directly to level 2.

## Design decision

> Isolation is a **feature**, not the core product. `daedalus`'s value
> proposition is *"one file, it runs"*. We start at level 0 (which proves
> the full pipeline) and add isolation **without changing the `.daedalus`
> format**: only the `isolation` metadata field and the launcher's behavior
> change.
