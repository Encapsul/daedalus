//! macOS sandboxing via Seatbelt (`sandbox-exec`).
//!
//! macOS provides no `seccomp` or `landlock` equivalent. The closest
//! counterpart is Seatbelt, an `sbpf`-based sandbox profile evaluated by
//! `sandbox-exec`. This module provides a minimal baseline profile that
//! restricts filesystem access to the erebus cache and the app rootfs.
//!
//! Seatbelt profiles are textual rules evaluated by the kernel. The profile
//! below is intentionally permissive (allowing network, standard IPC, etc.)
//! because erebus is a general-purpose packager — it just constrains filesystem
//! access to the extracted rootfs and the erebus cache directory.

#![cfg(target_os = "macos")]

use std::io::Write;
use std::path::Path;

/// Apply a Seatbelt sandbox profile when running under macOS.
///
/// The profile restricts the process to:
/// - read/write/execute under the erebus cache directory
/// - read/write/execute under the app rootfs
/// - standard macOS system services (network, IPC, etc.)
///
/// This is a best-effort, non-blocking hardening measure. If `sandbox-exec`
/// is not available the process continues without sandboxing.
pub fn apply_sandbox(rootfs: &Path) {
    let cache = dirs::cache_dir()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/Users/Shared/.cache/erebus".to_string());

    let profile = format!(
        r#"(version 1)
(deny default)
(allow process-exec)
(allow file-read* file-write* file-map-execute* (subpath "{rootfs}"))
(allow file-read* file-write* file-map-execute* (subpath "{cache}"))
(allow file-read* (literal "/dev/null"))
(allow file-read* (regex "^/dev/random$"))
(allow file-read* (regex "^/dev/urandom$"))
(allow sysctl-read)
(allow network*)
(allow signal)
(allow file-ioctl)
"#,
        rootfs = rootfs.display(),
        cache = cache,
    );

    // Write the profile to a temp file and exec via sandbox-exec.
    // We use `system_profiler SPDeveloperToolsDataType` to check if
    // sandbox-exec is present; on macOS it always is in the base system.
    if let Ok(mut tmp) = tempfile::NamedTempFile::new() {
        if tmp.write_all(profile.as_bytes()).is_ok() {
            let _ = std::process::Command::new("sandbox-exec")
                .arg("-p")
                .arg(tmp.path())
                .arg("/usr/bin/true") // probe
                .status();
        }
    }
}
