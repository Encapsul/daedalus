//! Landlock LSM sandboxing for filesystem access control.
//!
//! Implements Landlock (Linux 5.13+ — ABI v1) using raw syscalls, with an
//! ABI bump for `REFER` (v2, 5.19) and `TRUNCATE` (v3, 6.2). Restricts
//! filesystem access after `pivot_root`: full R/W on rootfs, read-only
//! everywhere else.

use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

// ---------------------------------------------------------------------------
// Landlock structs (ABI v1, Linux 5.13)
// ---------------------------------------------------------------------------

#[repr(C)]
struct landlock_ruleset_attr {
    handled_access_fs: u64,
}

const LANDLOCK_ACCESS_FS_EXECUTE: u64 = 1 << 0;
const LANDLOCK_ACCESS_FS_WRITE_FILE: u64 = 1 << 1;
const LANDLOCK_ACCESS_FS_READ_FILE: u64 = 1 << 2;
const LANDLOCK_ACCESS_FS_READ_DIR: u64 = 1 << 3;
const LANDLOCK_ACCESS_FS_REMOVE_DIR: u64 = 1 << 4;
const LANDLOCK_ACCESS_FS_REMOVE_FILE: u64 = 1 << 5;
const LANDLOCK_ACCESS_FS_MAKE_CHAR: u64 = 1 << 6;
const LANDLOCK_ACCESS_FS_MAKE_DIR: u64 = 1 << 7;
const LANDLOCK_ACCESS_FS_MAKE_REG: u64 = 1 << 8;
const LANDLOCK_ACCESS_FS_MAKE_SOCK: u64 = 1 << 9;
const LANDLOCK_ACCESS_FS_MAKE_FIFO: u64 = 1 << 10;
const LANDLOCK_ACCESS_FS_MAKE_BLOCK: u64 = 1 << 11;
const LANDLOCK_ACCESS_FS_MAKE_SYM: u64 = 1 << 12;
const LANDLOCK_ACCESS_FS_REFER: u64 = 1 << 13;
const LANDLOCK_ACCESS_FS_TRUNCATE: u64 = 1 << 14;

// All filesystem operations we deny by default (handled by the ruleset).
// ABI v3 (kernel 6.2+): includes REFER (v2) and TRUNCATE (v3).
const HANDLED_FS_V3: u64 = LANDLOCK_ACCESS_FS_EXECUTE
    | LANDLOCK_ACCESS_FS_WRITE_FILE
    | LANDLOCK_ACCESS_FS_READ_FILE
    | LANDLOCK_ACCESS_FS_READ_DIR
    | LANDLOCK_ACCESS_FS_REMOVE_DIR
    | LANDLOCK_ACCESS_FS_REMOVE_FILE
    | LANDLOCK_ACCESS_FS_MAKE_CHAR
    | LANDLOCK_ACCESS_FS_MAKE_DIR
    | LANDLOCK_ACCESS_FS_MAKE_REG
    | LANDLOCK_ACCESS_FS_MAKE_SOCK
    | LANDLOCK_ACCESS_FS_MAKE_FIFO
    | LANDLOCK_ACCESS_FS_MAKE_BLOCK
    | LANDLOCK_ACCESS_FS_MAKE_SYM
    | LANDLOCK_ACCESS_FS_REFER
    | LANDLOCK_ACCESS_FS_TRUNCATE;

// ABI v1 (kernel 5.13-6.1): landlock_create_ruleset rejects REFER/TRUNCATE
// with EINVAL, so strip them. Without this fallback the whole sandbox would
// fail-open or never install on older kernels.
const HANDLED_FS_V1: u64 =
    HANDLED_FS_V3 & !(LANDLOCK_ACCESS_FS_REFER | LANDLOCK_ACCESS_FS_TRUNCATE);

// Read-only access for everything outside the rootfs.
const READ_ONLY: u64 = LANDLOCK_ACCESS_FS_READ_FILE | LANDLOCK_ACCESS_FS_READ_DIR;

// Landlock uses the generic syscall numbers (444/445/446) on every
// architecture — x86_64, aarch64, 32-bit x86 and 32-bit ARM.
#[cfg(target_arch = "x86_64")]
mod sys {
    pub const LANDLOCK_CREATE_RULESET: libc::c_long = 444;
    pub const LANDLOCK_ADD_RULE: libc::c_long = 445;
    pub const LANDLOCK_RESTRICT_SELF: libc::c_long = 446;
}

#[cfg(target_arch = "aarch64")]
mod sys {
    pub const LANDLOCK_CREATE_RULESET: libc::c_long = 444;
    pub const LANDLOCK_ADD_RULE: libc::c_long = 445;
    pub const LANDLOCK_RESTRICT_SELF: libc::c_long = 446;
}

// Landlock uses the generic syscall numbers (444/445/446) on every
// architecture, including 32-bit x86 and ARM.
#[cfg(target_arch = "x86")]
mod sys {
    pub const LANDLOCK_CREATE_RULESET: libc::c_long = 444;
    pub const LANDLOCK_ADD_RULE: libc::c_long = 445;
    pub const LANDLOCK_RESTRICT_SELF: libc::c_long = 446;
}

#[cfg(target_arch = "arm")]
mod sys {
    pub const LANDLOCK_CREATE_RULESET: libc::c_long = 444;
    pub const LANDLOCK_ADD_RULE: libc::c_long = 445;
    pub const LANDLOCK_RESTRICT_SELF: libc::c_long = 446;
}

const LANDLOCK_RULE_PATH_BENEATH: u32 = 1;

#[repr(C)]
struct landlock_path_beneath_attr {
    allowed_access: u64,
    parent_fd: i32,
}

/// `O_PATH` handle to the extracted rootfs, opened *before* `pivot_root`.
///
/// Landlock rules are anchored to the inode a ruleset's `parent_fd` points
/// at, and the fd outlives the `pivot_root` mount-tree swap — so the rules
/// keep referring to the rootfs even though its original path no longer
/// resolves inside the new namespace.
pub struct RootfsGuard {
    fd: i32,
}

impl RootfsGuard {
    /// Opens `rootfs` with `O_PATH`. Must be called before `pivot_root_into`.
    pub fn open(rootfs: &Path) -> io::Result<Self> {
        let path_cstr = to_cstr(rootfs)?;
        // SAFETY: open(2) with O_PATH never executes or reads file contents;
        // path_cstr is a valid NUL-terminated path.
        let fd = unsafe { libc::open(path_cstr.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { fd })
    }
}

impl Drop for RootfsGuard {
    fn drop(&mut self) {
        // SAFETY: fd was returned by open(2) in Self::open and is still open;
        // close(2) on a valid fd is always safe.
        unsafe { libc::close(self.fd) };
    }
}

/// Install Landlock sandbox: rootfs gets full access, everything else is
/// read-only.
///
/// Must be called *after* `pivot_root` so that `/` already points to rootfs;
/// the rootfs rule uses the pre-opened fd from [`RootfsGuard`] instead of the
/// (now unresolvable) pre-pivot path.
///
/// Fail-closed: any error returns `Err` — the caller must not run the app
/// without the requested filesystem sandbox.
pub fn sandbox(root: &RootfsGuard) -> io::Result<()> {
    let (handled, ruleset_fd) = create_ruleset()?;

    // Allow full access to rootfs. `allowed_access` must be a subset of the
    // ruleset's handled set, so grant exactly the bits that were handled.
    add_fd_beneath(ruleset_fd, root.fd, handled)?;

    // Allow read-only everywhere else (after pivot_root, "/" is rootfs, so
    // this only matters for future bind mounts).
    add_path_beneath(ruleset_fd, Path::new("/"), READ_ONLY)?;

    // Enforce the ruleset on this process
    restrict_self(ruleset_fd)?;

    Ok(())
}

/// Create the ruleset, negotiating the highest supported ABI.
///
/// `landlock_create_ruleset` rejects handled bits the kernel does not know
/// (EINVAL). Try the full ABI-v3 set first (kernel 6.2+, which implies v2's
/// REFER), then fall back to the ABI-v1 set for 5.13-6.1. Any other error is
/// a hard failure (no Landlock support, seccomp'd environment, ...).
fn create_ruleset() -> io::Result<(u64, i32)> {
    match try_create(HANDLED_FS_V3) {
        Ok(fd) => Ok((HANDLED_FS_V3, fd)),
        Err(e) if e.raw_os_error() == Some(libc::EINVAL) => {
            // ABI v3 bits (REFER/TRUNCATE) unknown to kernels < 6.2; retry
            // with the ABI v1 set before giving up.
            let fd = try_create(HANDLED_FS_V1)?;
            Ok((HANDLED_FS_V1, fd))
        }
        Err(e) => Err(e),
    }
}

/// Single `landlock_create_ruleset` invocation for the given handled set.
fn try_create(handled: u64) -> io::Result<i32> {
    let attr = landlock_ruleset_attr {
        handled_access_fs: handled,
    };

    // SAFETY: landlock_create_ruleset is a Linux syscall; the attr struct
    // is valid and sized for ABI v1. Returns an fd or negative errno.
    let fd = unsafe {
        libc::syscall(
            sys::LANDLOCK_CREATE_RULESET,
            std::ptr::from_ref(&attr).cast::<libc::c_void>(),
            core::mem::size_of::<landlock_ruleset_attr>(),
            0u32,
        ) as i32
    };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(fd)
    }
}

/// Adds a path-beneath rule using an already-open `O_PATH` fd as the anchor.
fn add_fd_beneath(ruleset_fd: i32, parent_fd: i32, access: u64) -> io::Result<()> {
    let attr = landlock_path_beneath_attr {
        allowed_access: access,
        parent_fd,
    };

    // SAFETY: landlock_add_rule is a Linux syscall; parent_fd is a valid
    // O_PATH fd held by the caller, attr is a valid in-memory struct.
    let rc = unsafe {
        libc::syscall(
            sys::LANDLOCK_ADD_RULE,
            i64::from(ruleset_fd),
            i64::from(LANDLOCK_RULE_PATH_BENEATH),
            std::ptr::from_ref(&attr).cast::<libc::c_void>() as i64,
            0i64,
        ) as i32
    };

    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn add_path_beneath(ruleset_fd: i32, path: &Path, access: u64) -> io::Result<()> {
    let path_cstr = to_cstr(path)?;
    // SAFETY: open(2) with O_PATH is safe; path_cstr is a valid null-terminated string.
    let parent_fd = unsafe { libc::open(path_cstr.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
    if parent_fd < 0 {
        return Err(io::Error::last_os_error());
    }

    let result = add_fd_beneath(ruleset_fd, parent_fd, access);

    // Close parent_fd regardless of result
    // SAFETY: parent_fd is a valid file descriptor from open(2). close(2) is
    // always safe on a valid fd.
    unsafe { libc::close(parent_fd) };

    result
}

fn restrict_self(ruleset_fd: i32) -> io::Result<()> {
    // SAFETY: landlock_restrict_self is a Linux syscall. After this call
    // succeeds, no further Landlock restrictions can be added by this process.
    let rc =
        unsafe { libc::syscall(sys::LANDLOCK_RESTRICT_SELF, i64::from(ruleset_fd), 0i64) as i32 };
    unsafe { libc::close(ruleset_fd) };

    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn to_cstr(path: &Path) -> io::Result<std::ffi::CString> {
    std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains null byte"))
}
