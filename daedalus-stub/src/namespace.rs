//! Namespace setup for the daedalus launcher stub.
//!
//! Provides Linux user + mount namespace isolation (`enter_userns`,
//! `pivot_root_into`) and container detection (`running_in_container`).
//! All functions here are Linux-only and gated with `#[cfg(target_os = "linux")]`.

use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

/// Detect if the launcher is running inside a known container environment.
///
/// Checks `/.dockerenv`, `/run/.containerenv`, and cgroup v1/v2 hints.
/// Returns `true` when a container runtime is detected.
///
/// This is a best-effort hint, not a security boundary: a compromised
/// container can fake these files. Combined with namespace isolation and
/// Landlock/seccomp, it raises the bar against container escapes.
#[cfg(target_os = "linux")]
pub fn running_in_container() -> bool {
    const CGROUP_HINTS: [&str; 3] = ["/.dockerenv", "/run/.containerenv", "/.containerenv"];
    CGROUP_HINTS.iter().any(|p| Path::new(p).exists())
        || std::fs::read_to_string("/proc/1/cgroup")
            .ok()
            .is_some_and(|c| {
                c.contains("docker") || c.contains("kubepod") || c.contains("containerd")
            })
        || std::fs::read_to_string("/proc/self/cgroup")
            .ok()
            .is_some_and(|c| {
                c.contains("docker") || c.contains("kubepod") || c.contains("containerd")
            })
}

/// Enter a new user + mount namespace (unprivileged). Linux-only.
#[cfg(target_os = "linux")]
pub fn enter_userns() -> io::Result<()> {
    // SAFETY: getuid(2) and getgid(2) are always safe, returning the
    // caller's UID/GID.
    let uid = unsafe { libc::getuid() };
    let gid = unsafe { libc::getgid() };

    // SAFETY: unshare(2) with CLONE_NEWUSER|CLONE_NEWNS creates a new user
    // and mount namespace. CLONE_NEWUSER does not require CAP_SYS_ADMIN.
    // The uid_map/gid_map writes below grant full UID/GID mapping.
    unsafe {
        let rc = libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNS);
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
    }

    write_proc("/proc/self/uid_map", &format!("0 {uid} 1"))?;
    write_proc("/proc/self/setgroups", "deny")?;
    write_proc("/proc/self/gid_map", &format!("0 {gid} 1"))?;

    Ok(())
}

/// Write a procfs file (`uid_map`/`gid_map`). Linux-only.
#[cfg(target_os = "linux")]
pub fn write_proc(path: &str, contents: &str) -> io::Result<()> {
    let mut f = File::create(path)?;
    f.write_all(contents.as_bytes())?;
    Ok(())
}
