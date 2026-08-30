//! Seccomp BPF sandbox for the daedalus launcher stub.
//!
//! Provides `install_seccomp_denylist()` (always-deny 18 dangerous syscalls)
//! and `install_seccomp_with_capabilities()` (extends the denylist based on
//! the artifact's declared `Capability` set). Linux-only.

#[cfg(target_os = "linux")]
use std::io;

#[cfg(target_os = "linux")]
use daedalus_core::layer::Capability;

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
    #[cfg(target_arch = "riscv64")]
    pub const AUDIT_ARCH: u32 = 0xC000_00F3;
}

#[cfg(target_os = "linux")]
use arch_consts::AUDIT_ARCH;

/// Install a seccomp-bpf denylist (backward compatible — no capabilities).
///
/// Equivalent to calling `install_seccomp_with_capabilities(&[])` — denies
/// the 18 always-dangerous syscalls with no capability-based additions.
#[cfg(target_os = "linux")]
#[allow(dead_code)]
/// `install_seccomp_denylist` - install seccomp denylist.
/// `@io`: io
///
/// Description:
///
/// Return: Result containing `io::Result<()>`
pub fn install_seccomp_denylist() -> io::Result<()> {
    install_seccomp_with_capabilities(&[])
}

/// Install a capability-driven seccomp filter.
///
/// Always denies the 18 dangerous syscalls from `always_deny_syscalls()`.
/// Additionally:
/// - Denies network syscalls when `Capability::Network` is absent.
/// - Denies exec-family syscalls when `Capability::Exec` is absent.
///
/// **Exec capability limitation**: blocking `execve` would prevent the stub
/// from launching the entrypoint via `execvp` because the filter is installed
/// before `execvp`. When `Capability::Exec` is absent and seccomp is active,
/// `exec_app` must `fork()` before calling `execvp` — the child inherits the
/// execve-deny filter, its `execvp` fails with `SECCOMP_RET_KILL_PROCESS`,
/// and the parent detects the child's exit to refuse the launch cleanly.
#[cfg(target_os = "linux")]
/// `install_seccomp_with_capabilities` - install seccomp with capabilities.
/// `@capabilities`: capabilities
/// `@io`: io
///
/// Description:
///
/// Return: Result containing `io::Result<()>`
pub fn install_seccomp_with_capabilities(capabilities: &[Capability]) -> io::Result<()> {
    let mut syscalls = always_deny_syscalls();
    let has_network = capabilities.contains(&Capability::Network);
    if !has_network {
        syscalls.extend_from_slice(network_syscalls());
    }
    let has_exec = capabilities.contains(&Capability::Exec);
    if !has_exec {
        syscalls.extend_from_slice(exec_syscalls());
    }
    let filter = build_seccomp_filter_for_syscalls(&syscalls);
    let prog = libc::sock_fprog {
        len: filter.len() as u16,
        filter: filter.as_ptr().cast_mut(),
    };

    // SAFETY: prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &prog) installs a BPF
    // filter on the current process. The filter program is a stack-allocated
    // Vec that outlives the prctl call. prctl only reads the filter.
    // On success the filter is permanent — blocked syscalls SIGSYS.
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

/// Syscalls blocked unconditionally (no capability can enable them).
#[cfg(target_os = "linux")]
/// `always_deny_syscalls` - always deny syscalls.
///
/// Description:
///
/// Return: vector of Vec<u32>
fn always_deny_syscalls() -> Vec<u32> {
    let mut v = vec![
        libc::SYS_ptrace as u32,
        libc::SYS_bpf as u32,
        libc::SYS_userfaultfd as u32,
        libc::SYS_mount as u32,
        libc::SYS_umount2 as u32,
        libc::SYS_pivot_root as u32,
        libc::SYS_reboot as u32,
        libc::SYS_sethostname as u32,
        libc::SYS_setdomainname as u32,
        libc::SYS_swapon as u32,
        libc::SYS_swapoff as u32,
        libc::SYS_acct as u32,
        libc::SYS_kexec_load as u32,
        libc::SYS_init_module as u32,
        libc::SYS_finit_module as u32,
        libc::SYS_delete_module as u32,
        libc::SYS_nfsservctl as u32,
    ];
    // kexec_file_load is x86_64-only; other arches reuse kexec_load above.
    #[cfg(target_arch = "x86_64")]
    {
        v.push(libc::SYS_kexec_file_load as u32);
    }
    v
}

/// Syscalls blocked when `Capability::Network` is absent.
#[cfg(target_os = "linux")]
/// `network_syscalls` - network syscalls.
///
/// Description:
///
/// Return: the &'static [u32]
fn network_syscalls() -> &'static [u32] {
    &[
        libc::SYS_socket as u32,
        libc::SYS_socketpair as u32,
        libc::SYS_connect as u32,
        libc::SYS_bind as u32,
        libc::SYS_listen as u32,
        libc::SYS_accept as u32,
        libc::SYS_accept4 as u32,
        libc::SYS_sendto as u32,
        libc::SYS_recvfrom as u32,
        libc::SYS_sendmsg as u32,
        libc::SYS_recvmsg as u32,
        libc::SYS_getsockname as u32,
        libc::SYS_getpeername as u32,
        libc::SYS_setsockopt as u32,
        libc::SYS_getsockopt as u32,
        libc::SYS_shutdown as u32,
    ]
}

/// Exec-family syscalls blocked when `Capability::Exec` is absent.
///
/// `execve` launches a new program; `execveat` is the `at`-family variant.
/// `clone`, `clone3`, `fork`, and `vfork` create subprocesses — without `Exec`
/// an application should not be able to spawn children at all. `posix_spawn`
/// internally calls `execve`/`clone`, so it is covered transitively.
#[cfg(target_os = "linux")]
/// `exec_syscalls` - exec syscalls.
///
/// Description:
///
/// Return: the &'static [u32]
fn exec_syscalls() -> &'static [u32] {
    #[cfg(any(target_arch = "aarch64", target_arch = "riscv64"))]
    {
        &[
            libc::SYS_execve as u32,
            libc::SYS_execveat as u32,
            libc::SYS_clone as u32,
            libc::SYS_clone3 as u32,
        ]
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "riscv64")))]
    {
        &[
            libc::SYS_execve as u32,
            libc::SYS_execveat as u32,
            libc::SYS_clone as u32,
            libc::SYS_clone3 as u32,
            libc::SYS_fork as u32,
            libc::SYS_vfork as u32,
        ]
    }
}

/// Build a seccomp BPF filter denying the given syscall numbers.
#[cfg(target_os = "linux")]
/// `build_seccomp_filter_for_syscalls` - build seccomp filter for syscalls.
/// `@syscalls`: syscalls
/// `@libc`: libc
///
/// Description:
///
/// Return: vector of Vec<libc::sock_filter>
fn build_seccomp_filter_for_syscalls(syscalls: &[u32]) -> Vec<libc::sock_filter> {
    const BPF_LD: u16 = 0x00;
    const BPF_W: u16 = 0x00;
    const BPF_ABS: u16 = 0x20;
    const BPF_JMP: u16 = 0x05;
    const BPF_JEQ: u16 = 0x10;
    const BPF_RET: u16 = 0x06;
    const BPF_K: u16 = 0x00;

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

    let n = syscalls.len();
    let kill_idx = 3 + n;
    let allow_idx = kill_idx + 1;

    let mut filter: Vec<libc::sock_filter> = Vec::with_capacity(allow_idx + 1);
    filter.push(arch_load(BPF_LD, 0, 0, 4));
    filter.push(jmp_eq(AUDIT_ARCH, 0, (allow_idx - 2) as u8));
    filter.push(arch_load(BPF_LD, 0, 0, 0));
    for (i, &sys) in syscalls.iter().enumerate() {
        let idx = 3 + i;
        let jump_to_kill = (kill_idx - 1 - idx) as u8;
        let jump_to_next = if i == n - 1 {
            (allow_idx - 1 - idx) as u8
        } else {
            0
        };
        filter.push(jmp_eq(sys, jump_to_kill, jump_to_next));
    }
    filter.push(ret(SECCOMP_RET_KILL_PROCESS));
    filter.push(ret(SECCOMP_RET_ALLOW));
    filter
}

/// Build the seccomp BPF denylist program for the current architecture.
///
/// Factored out from `install_seccomp_denylist` so the program structure can be
/// validated without installing it (see `validate_seccomp_filter`). The program
/// is a denylist: matched dangerous syscalls return `SECCOMP_RET_KILL_PROCESS`;
/// everything else — including arch mismatches — falls through to
/// `SECCOMP_RET_ALLOW`.
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub(crate) fn build_seccomp_filter() -> Vec<libc::sock_filter> {
    build_seccomp_filter_for_syscalls(&always_deny_syscalls())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    const BPF_RET: u16 = 0x06;
    const BPF_K: u16 = 0x00;

    #[test]
    /// `validate_seccomp_filter_is_denylist_not_denyall` - validate seccomp filter is denylist not denyall.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn validate_seccomp_filter_is_denylist_not_denyall() {
        let filter = build_seccomp_filter();
        let n = always_deny_syscalls().len();
        let kill_idx = 3 + n;
        let allow_idx = kill_idx + 1;

        // n syscall checks + arch load + arch jmp + syscall load + kill + allow.
        assert_eq!(
            filter.len(),
            allow_idx + 1,
            "filter instruction count changed"
        );

        // THE critical "not deny-all" invariant: the final fallthrough return
        // must be ALLOW, not KILL_PROCESS. If this ever flips, every syscall
        // is blocked and the launcher can no longer exec anything.
        let allow = &filter[allow_idx];
        assert_eq!(allow.code, BPF_RET | BPF_K);
        assert_eq!(
            allow.k,
            libc::SECCOMP_RET_ALLOW,
            "last instruction must fall through to ALLOW (denylist, not deny-all)"
        );

        // The deny target (reached on a matched dangerous syscall) must actually
        // kill: KILL_PROCESS is 0x8000_0000 (non-zero, not a silent no-op).
        let kill = &filter[kill_idx];
        assert_eq!(kill.code, BPF_RET | BPF_K);
        assert_eq!(kill.k, libc::SECCOMP_RET_KILL_PROCESS);
        assert_ne!(kill.k, 0, "KILL target must be a real kill, not a no-op");

        // Architecture-mismatch path (filter[1], jf=N) must land on ALLOW.
        let arch_mismatch_land = 1 + 1 + filter[1].jf as usize;
        assert_eq!(arch_mismatch_land, allow_idx);
        assert_eq!(filter[arch_mismatch_land].k, libc::SECCOMP_RET_ALLOW);

        // Each syscall-deny jump must land on the KILL target.
        for (idx, insn) in filter.iter().enumerate().take(kill_idx).skip(3) {
            if insn.code & 0x05 != 0x05 {
                continue;
            }
            let land = idx + 1 + insn.jt as usize;
            assert_eq!(
                land, kill_idx,
                "deny jump at idx {idx} must reach KILL (idx {kill_idx}), not {land}"
            );
        }

        // The documented dangerous syscalls are all present.
        let blocked: Vec<u32> = filter
            .iter()
            .enumerate()
            .take(kill_idx)
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

        assert!(u16::try_from(filter.len()).is_ok());
    }

    #[test]
    /// `capability_filter_adds_network_deny_without_network_cap` - capability filter adds network deny without network cap.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn capability_filter_adds_network_deny_without_network_cap() {
        let filter = build_seccomp_filter_for_syscalls(&{
            let mut v = always_deny_syscalls();
            v.extend_from_slice(network_syscalls());
            v
        });
        let blocked: Vec<u32> = filter
            .iter()
            .enumerate()
            .skip(3)
            .take_while(|(_, i)| i.code & 0x05 == 0x05)
            .map(|(_, i)| i.k)
            .collect();
        assert!(
            blocked.contains(&(libc::SYS_socket as u32)),
            "network syscall (socket) must be blocked when Network cap absent"
        );
        assert!(
            blocked.contains(&(libc::SYS_connect as u32)),
            "network syscall (connect) must be blocked"
        );
    }

    #[test]
    /// `capability_filter_without_network_cap_is_larger` - capability filter without network cap is larger.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn capability_filter_without_network_cap_is_larger() {
        let base = build_seccomp_filter();
        let with_caps = {
            let mut v = always_deny_syscalls();
            v.extend_from_slice(network_syscalls());
            build_seccomp_filter_for_syscalls(&v)
        };
        assert!(
            with_caps.len() > base.len(),
            "filter without Network cap should have more deny entries than base"
        );
    }

    #[test]
    /// `capability_filter_adds_exec_deny_without_exec_cap` - capability filter adds exec deny without exec cap.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn capability_filter_adds_exec_deny_without_exec_cap() {
        let filter = build_seccomp_filter_for_syscalls(&{
            let mut v = always_deny_syscalls();
            v.extend_from_slice(exec_syscalls());
            v
        });
        let blocked: Vec<u32> = filter
            .iter()
            .enumerate()
            .skip(3)
            .take_while(|(_, i)| i.code & 0x05 == 0x05)
            .map(|(_, i)| i.k)
            .collect();
        assert!(
            blocked.contains(&(libc::SYS_execve as u32)),
            "execve must be blocked when Exec cap absent"
        );
        assert!(
            blocked.contains(&(libc::SYS_clone as u32)),
            "clone must be blocked when Exec cap absent"
        );
    }

    #[test]
    /// `exec_syscalls_list_is_arch_dependent` - exec syscalls list is arch dependent.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn exec_syscalls_list_is_arch_dependent() {
        let execs = exec_syscalls();
        assert!(!execs.is_empty(), "exec syscall list must not be empty");
        assert!(
            execs.contains(&(libc::SYS_execve as u32)),
            "execve must always be in exec_syscalls"
        );
    }
}
