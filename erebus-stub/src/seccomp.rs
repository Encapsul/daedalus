//! Seccomp BPF denylist for the xbin launcher stub.
//!
//! Provides `install_seccomp_denylist()` which installs a conservative BPF
//! filter after `pivot_root`. Linux-only.

#[cfg(target_os = "linux")]
use std::io;

#[cfg(target_os = "linux")]
mod arch_consts {
    #[cfg(target_arch = "x86_64")]
    pub const AUDIT_ARCH: u32 = 0xC000_003E;
    #[cfg(target_arch = "aarch64")]
    pub const AUDIT_ARCH: u32 = 0xC000_00B7;
    #[cfg(target_arch = "x86")]
    pub const AUDIT_ARCH: u32 = 0x4000_0003;
    #[cfg(target_arch = "arm")]
    pub const AUDIT_ARCH: u32 = 0x4000_0028;
}

#[cfg(target_os = "linux")]
use arch_consts::AUDIT_ARCH;

/// Install a seccomp-bpf denylist after `pivot_root`. Linux-only.
///
/// Blocks syscalls that have no legitimate use in a packaged web/server app
/// and represent escalation paths not covered by namespace isolation.
/// The list is conservative: 18 syscalls, all clearly dangerous.
/// Apps that work without seccomp continue working with it.
#[cfg(target_os = "linux")]
pub fn install_seccomp_denylist() -> io::Result<()> {
    let filter = build_seccomp_filter();
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

/// Build the seccomp BPF denylist program for the current architecture.
///
/// Factored out from `install_seccomp_denylist` so the program structure can be
/// validated without installing it (see `validate_seccomp_filter`). The program
/// is a denylist: matched dangerous syscalls return `SECCOMP_RET_KILL_PROCESS`;
/// everything else — including arch mismatches — falls through to
/// `SECCOMP_RET_ALLOW`.
#[cfg(target_os = "linux")]
pub(crate) fn build_seccomp_filter() -> Vec<libc::sock_filter> {
    // BPF instruction encodings (linux/filter.h).
    const BPF_LD: u16 = 0x00;
    const BPF_W: u16 = 0x00;
    const BPF_ABS: u16 = 0x20;
    const BPF_JMP: u16 = 0x05;
    const BPF_JEQ: u16 = 0x10;
    const BPF_RET: u16 = 0x06;
    const BPF_K: u16 = 0x00;

    // kexec_file_load is x86_64-only; other arches reuse kexec_load.
    #[cfg(target_arch = "x86_64")]
    const SYS_KEXEC_FILE_LOAD: u32 = libc::SYS_kexec_file_load as u32;
    #[cfg(not(target_arch = "x86_64"))]
    const SYS_KEXEC_FILE_LOAD: u32 = libc::SYS_kexec_load as u32;

    const SYS_PTRACE: u32 = libc::SYS_ptrace as u32;
    const SYS_BPF: u32 = libc::SYS_bpf as u32;
    const SYS_USERFAULTFD: u32 = libc::SYS_userfaultfd as u32;
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

    // Seccomp return codes — use libc constants (stable kernel ABI values).
    const SECCOMP_RET_KILL_PROCESS: u32 = libc::SECCOMP_RET_KILL_PROCESS;
    const SECCOMP_RET_ALLOW: u32 = libc::SECCOMP_RET_ALLOW;

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
        arch_load(BPF_LD, 0, 0, 4),        //  0: load architecture
        jmp_eq(AUDIT_ARCH, 0, 20),         //  1: arch mismatch -> idx 22 (ALLOW)
        arch_load(BPF_LD, 0, 0, 0),        //  2: load syscall number
        jmp_eq(SYS_PTRACE, 17, 0),         //  3: match -> idx 21 (KILL)
        jmp_eq(SYS_BPF, 16, 0),            //  4
        jmp_eq(SYS_USERFAULTFD, 15, 0),    //  5
        jmp_eq(SYS_MOUNT, 14, 0),          //  6
        jmp_eq(SYS_UMOUNT2, 13, 0),        //  7
        jmp_eq(SYS_PIVOT_ROOT, 12, 0),     //  8
        jmp_eq(SYS_REBOOT, 11, 0),         //  9
        jmp_eq(SYS_SETHOSTNAME, 10, 0),    // 10
        jmp_eq(SYS_SETDOMAINNAME, 9, 0),   // 11
        jmp_eq(SYS_SWAPON, 8, 0),          // 12
        jmp_eq(SYS_SWAPOFF, 7, 0),         // 13
        jmp_eq(SYS_ACCT, 6, 0),            // 14
        jmp_eq(SYS_NFSSERVCTL, 5, 0),      // 15
        jmp_eq(SYS_KEXEC_LOAD, 4, 0),      // 16
        jmp_eq(SYS_INIT_MODULE, 3, 0),     // 17
        jmp_eq(SYS_FINIT_MODULE, 2, 0),    // 18
        jmp_eq(SYS_DELETE_MODULE, 1, 0),   // 19
        jmp_eq(SYS_KEXEC_FILE_LOAD, 0, 1), // 20: no match -> idx 22 (ALLOW)
        ret(SECCOMP_RET_KILL_PROCESS),     // 21: deny target
        ret(SECCOMP_RET_ALLOW),            // 22: allow fallthrough
    ];

    filter
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// BPF opcode masks for validation.
    const BPF_RET: u16 = 0x06;
    const BPF_K: u16 = 0x00;

    #[test]
    fn validate_seccomp_filter_is_denylist_not_denyall() {
        let filter = build_seccomp_filter();

        // 18 syscall checks + arch load + arch jmp + syscall load + kill + allow.
        assert_eq!(filter.len(), 23, "filter instruction count changed");

        // THE critical "not deny-all" invariant: the final fallthrough return
        // must be ALLOW, not KILL_PROCESS. If this ever flips, every syscall
        // is blocked and the launcher can no longer exec anything.
        let allow = &filter[22];
        assert_eq!(allow.code, BPF_RET | BPF_K);
        assert_eq!(
            allow.k,
            libc::SECCOMP_RET_ALLOW,
            "last instruction must fall through to ALLOW (denylist, not deny-all)"
        );

        // The deny target (reached on a matched dangerous syscall) must actually
        // kill: KILL_PROCESS is 0x8000_0000 (non-zero, not a silent no-op).
        let kill = &filter[21];
        assert_eq!(kill.code, BPF_RET | BPF_K);
        assert_eq!(kill.k, libc::SECCOMP_RET_KILL_PROCESS);
        assert_ne!(kill.k, 0, "KILL target must be a real kill, not a no-op");

        // Architecture-mismatch path (filter[1], jf=20) must land on ALLOW (idx 22),
        // i.e. cross-arch/mismatched syscalls are permitted rather than killed.
        // On BPF_JEQ: equal -> jt, not-equal -> jf. Mismatch is the not-equal
        // branch, so we use jf.
        let arch_mismatch_land = 1 + 1 + filter[1].jf as usize;
        assert_eq!(arch_mismatch_land, 22);
        assert_eq!(filter[arch_mismatch_land].k, libc::SECCOMP_RET_ALLOW);

        // Each syscall-deny jump must land on the KILL target (idx 21): for
        // instruction at index n, jt means "skip jt instructions on match",
        // landing at n + 1 + jt.
        for (idx, insn) in filter.iter().enumerate().take(21).skip(3) {
            // Only the BPF_JMP instructions carry the syscall comparisons.
            if insn.code & 0x05 != 0x05 {
                continue;
            }
            let land = idx + 1 + insn.jt as usize;
            assert_eq!(
                land, 21,
                "deny jump at idx {idx} must reach KILL (idx 21), not {land}"
            );
        }

        // The documented dangerous syscalls are all present.
        let blocked: Vec<u32> = filter
            .iter()
            .enumerate()
            .take(21)
            .skip(3)
            .filter(|(_, i)| i.code & 0x05 == 0x05)
            .map(|(_, i)| i.k)
            .collect();
        for want in [
            libc::SYS_ptrace as u32,
            libc::SYS_bpf as u32,
            libc::SYS_userfaultfd as u32,
            libc::SYS_mount as u32,
            libc::SYS_pivot_root as u32,
        ] {
            assert!(blocked.contains(&want), "missing denylist entry: {want}");
        }

        // BPF program length fits in the u16 `len` field sock_fprog uses.
        assert!(u16::try_from(filter.len()).is_ok());
    }
}
