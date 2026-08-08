//! Seccomp BPF denylist for the xbin launcher stub.
//!
//! Provides `install_seccomp_denylist()` which installs a conservative BPF
//! filter after `pivot_root`. Linux-only.

use std::io;

/// Install a seccomp-bpf denylist after `pivot_root`. Linux-only.
///
/// Blocks syscalls that have no legitimate use in a packaged web/server app
/// and represent escalation paths not covered by namespace isolation.
/// The list is conservative: only ~14 syscalls, all clearly dangerous.
/// Apps that work without seccomp continue working with it.
#[cfg(target_os = "linux")]
pub fn install_seccomp_denylist() -> io::Result<()> {
    use std::io;
    // BPF instruction encodings (linux/filter.h).
    const BPF_LD: u16 = 0x00;
    const BPF_W: u16 = 0x00;
    const BPF_ABS: u16 = 0x20;
    const BPF_JMP: u16 = 0x05;
    const BPF_JEQ: u16 = 0x10;
    const BPF_RET: u16 = 0x06;
    const BPF_K: u16 = 0x00;

    /// `seccomp_data.arch` is at offset 4, `seccomp_data.nr` is at offset 0.
    const SECCOMP_RET_KILL_PROCESS: u32 = 0x0002_0000;
    const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;

    // Audit arch — differs between x86_64 and aarch64.
    // libc does not expose AUDIT_ARCH, so we keep these values here.
    // They are stable kernel ABI constants.
    #[cfg(target_arch = "x86_64")]
    const AUDIT_ARCH: u32 = 0xC000_003E;
    #[cfg(target_arch = "aarch64")]
    const AUDIT_ARCH: u32 = 0xC000_00B7;
    #[cfg(target_arch = "x86")]
    const AUDIT_ARCH: u32 = 0x4000_0003;
    #[cfg(target_arch = "arm")]
    const AUDIT_ARCH: u32 = 0x4000_0028;

    // Syscall numbers — use libc constants instead of hardcoded values.
    // libc exposes the correct per-arch syscall numbers via cfg(target_arch).
    // kexec_file_load is x86_64-only; other arches reuse kexec_load.
    #[cfg(target_arch = "x86_64")]
    const SYS_KEXEC_FILE_LOAD: u32 = libc::SYS_kexec_file_load as u32;
    #[cfg(not(target_arch = "x86_64"))]
    const SYS_KEXEC_FILE_LOAD: u32 = libc::SYS_kexec_load as u32;

    const SYS_PTRACE: u32 = libc::SYS_ptrace as u32;
    const SYS_MOUNT: u32 = libc::SYS_mount as u32;
    const SYS_UMOUNT2: u32 = libc::SYS_umount2 as u32;
    const SYS_PIVOT_ROOT: u32 = libc::SYS_pivot_root as u32;
    const SYS_REBOOT: u32 = libc::SYS_reboot as u32;
    const SYS_SETHOSTNAME: u32 = libc::SYS_sethostname as u32;
    const SYS_SETDOMAINNAME: u32 = libc::SYS_setdomainname as u32;
    const SYS_SWAPON: u32 = libc::SYS_swapon as u32;
    const SYS_SWAPOFF: u32 = libc::SYS_swapoff as u32;
    const SYS_ACCT: u32 = libc::SYS_acct as u32;
    const SYS_KEXEC_LOAD: u32 = libc::SYS_kexec_load as u32;
    const SYS_INIT_MODULE: u32 = libc::SYS_init_module as u32;
    const SYS_FINIT_MODULE: u32 = libc::SYS_finit_module as u32;
    const SYS_DELETE_MODULE: u32 = libc::SYS_delete_module as u32;
    const SYS_NFSSERVCTL: u32 = libc::SYS_nfsservctl as u32;

    let arch_load = |code: u16, jt: u8, jf: u8, k: u32| libc::sock_filter {
        code: code | BPF_W | BPF_ABS,
        jt,
        jf,
        k,
    };
    let jmp_eq = |k: u32, jt: u8, jf: u8| libc::sock_filter {
        code: BPF_JMP | BPF_JEQ | BPF_K,
        jt,
        jf,
        k,
    };
    let ret = |k: u32| libc::sock_filter {
        code: BPF_RET | BPF_K,
        jt: 0,
        jf: 0,
        k,
    };

    #[allow(clippy::similar_names)]
    let filter: Vec<libc::sock_filter> = vec![
        arch_load(BPF_LD, 0, 0, 4),
        jmp_eq(AUDIT_ARCH, 0, 18),
        arch_load(BPF_LD, 0, 0, 0),
        jmp_eq(SYS_PTRACE, 15, 0),
        jmp_eq(SYS_MOUNT, 14, 0),
        jmp_eq(SYS_UMOUNT2, 13, 0),
        jmp_eq(SYS_PIVOT_ROOT, 12, 0),
        jmp_eq(SYS_REBOOT, 11, 0),
        jmp_eq(SYS_SETHOSTNAME, 10, 0),
        jmp_eq(SYS_SETDOMAINNAME, 9, 0),
        jmp_eq(SYS_SWAPON, 8, 0),
        jmp_eq(SYS_SWAPOFF, 7, 0),
        jmp_eq(SYS_ACCT, 6, 0),
        jmp_eq(SYS_NFSSERVCTL, 5, 0),
        jmp_eq(SYS_KEXEC_LOAD, 4, 0),
        jmp_eq(SYS_INIT_MODULE, 3, 0),
        jmp_eq(SYS_FINIT_MODULE, 2, 0),
        jmp_eq(SYS_DELETE_MODULE, 1, 0),
        jmp_eq(SYS_KEXEC_FILE_LOAD, 0, 0),
        ret(SECCOMP_RET_KILL_PROCESS),
        ret(SECCOMP_RET_ALLOW),
    ];

    let prog = libc::sock_fprog {
        len: filter.len() as u16,
        filter: filter.as_ptr().cast_mut(),
    };

    // SAFETY: prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &prog) installs
    // a BPF filter on the current process. The filter program and its data
    // are stack-allocated Vec that outlive the prctl call. On success the
    // filter is permanent — any blocked syscall kills the process with SIGSYS.
    let rc = unsafe {
        libc::prctl(
            libc::PR_SET_SECCOMP,
            2,
            std::ptr::from_ref(&prog) as usize,
            0,
            0,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
