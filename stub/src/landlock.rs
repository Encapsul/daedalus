//! Landlock LSM sandboxing for filesystem access control.
//!
//! Implements Landlock ABI v1 (Linux 5.13+) using raw syscalls.
//! Restricts filesystem access after `pivot_root`: full R/W on rootfs,
//! read-only everywhere else.

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
const LANDLOCK_ACCESS_FS_TRUNCATE: u64 = 1 << 14;

// All filesystem operations we deny by default (handled by the ruleset).
const HANDLED_FS: u64 = LANDLOCK_ACCESS_FS_EXECUTE
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
    | LANDLOCK_ACCESS_FS_TRUNCATE;

// Full read-write access for the rootfs exception.
const FULL_RW: u64 = HANDLED_FS;

// Read-only access for everything else.
const READ_ONLY: u64 =
    LANDLOCK_ACCESS_FS_EXECUTE | LANDLOCK_ACCESS_FS_READ_FILE | LANDLOCK_ACCESS_FS_READ_DIR;

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

/// Install Landlock sandbox: rootfs gets full access, everything else is read-only.
///
/// Must be called *after* `pivot_root` so that `/` already points to rootfs.
pub fn sandbox(rootfs: &Path) -> io::Result<()> {
    let ruleset_fd = create_ruleset()?;

    // Allow full access to rootfs
    add_path_beneath(ruleset_fd, rootfs, FULL_RW)?;

    // Allow read-only everywhere else (after pivot_root, "/" is rootfs,
    // so this only matters for future bind mounts or if pivot_root fails)
    add_path_beneath(ruleset_fd, Path::new("/"), READ_ONLY)?;

    // Enforce the ruleset on this process
    restrict_self(ruleset_fd)?;

    Ok(())
}

fn create_ruleset() -> io::Result<i32> {
    let attr = landlock_ruleset_attr {
        handled_access_fs: HANDLED_FS,
    };

    // SAFETY: landlock_create_ruleset is a Linux syscall. The attr struct
    // is valid and sized correctly for ABI v1. Returns a file descriptor
    // or negative errno.
    let fd = unsafe {
        libc::syscall(
            sys::LANDLOCK_CREATE_RULESET,
            std::ptr::from_ref(&attr).cast::<libc::c_void>(),
            core::mem::size_of::<landlock_ruleset_attr>(),
            0u32,
        ) as i32
    };

    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(fd)
}

fn add_path_beneath(ruleset_fd: i32, path: &Path, access: u64) -> io::Result<()> {
    let path_cstr = to_cstr(path)?;
    // SAFETY: open(2) with O_PATH is safe; path_cstr is a valid null-terminated string.
    let parent_fd = unsafe { libc::open(path_cstr.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
    if parent_fd < 0 {
        return Err(io::Error::last_os_error());
    }

    let attr = landlock_path_beneath_attr {
        allowed_access: access,
        parent_fd,
    };

    // SAFETY: landlock_add_rule is a Linux syscall.
    let rc = unsafe {
        libc::syscall(
            sys::LANDLOCK_ADD_RULE,
            i64::from(ruleset_fd),
            i64::from(LANDLOCK_RULE_PATH_BENEATH),
            std::ptr::from_ref(&attr).cast::<libc::c_void>() as i64,
            0i64,
        ) as i32
    };

    // Close parent_fd regardless of result
    unsafe { libc::close(parent_fd) };

    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
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
