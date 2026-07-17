//! xbin launcher stub.
//!
//! Embedded at the head of every .xbin file — this is the ELF the kernel runs.
//! Flow: open /proc/self/exe → read footer → verify integrity (sig → SHA-256) →
//! extract rootfs to ~/.cache/xbin/{sha256}/ (atomic) → exec the app.
//!
//! Level 0 isolation (MVP): LD_LIBRARY_PATH, no chroot. Levels 1/2
//! (chroot, user namespaces) in Phase 2 — see docs/src/roadmap.md.

mod format;

use format::{read_at, Footer};
use serde::Deserialize;
use std::ffi::CString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::exit;

/// Standard library search paths for LD_LIBRARY_PATH (x86_64 Linux).
/// Kept in sync with cli/xbin/build.py LD_LIBRARY_PATH construction.
const LD_PATHS: &[&str] = &[
    "lib", "lib64", "usr/lib", "usr/lib64", "usr/lib/x86_64-linux-gnu",
];

/// Binary search paths for PATH, mirroring LD_PATHS for executables.
/// Bundled binaries (e.g. ffmpeg, gitleaks) land here via the rootfs.
const BIN_PATHS: &[&str] = &["usr/bin", "bin", "usr/local/bin"];

#[derive(Deserialize)]
struct Metadata {
    name: String,
    #[serde(default)]
    runtime: String,
    #[serde(default)]
    entrypoint: Vec<String>,
    #[serde(default)]
    env: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    layers: Vec<Layer>,
    #[serde(default)]
    isolation: u8,
    #[serde(default)]
    services: Vec<Service>,
}

#[derive(Deserialize)]
struct Service {
    name: String,
    cmd: Vec<String>,
    #[serde(default)]
    env: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    ready_port: u16,
    #[serde(default)]
    ready_timeout: u64,
}

#[derive(Deserialize)]
struct Layer {
    #[serde(default)]
    kind: String,
    offset: u64,
    csize: u64,
    #[serde(rename = "usize")]
    uncompressed_size: u64,
    sha256: String,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("[xbin] error: {e}");
        exit(1);
    }
}

fn run() -> io::Result<()> {
    let verbose = std::env::var_os("XBIN_VERBOSE").is_some();

    // 1. Read footer + metadata (small, fast).
    let mut exe = File::open("/proc/self/exe")?;
    let footer = Footer::read_from(&mut exe)?;
    let meta_bytes = read_at(&mut exe, footer.meta_offset, footer.meta_size as usize)?;
    let meta: Metadata = serde_json::from_slice(&meta_bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("bad metadata: {e}")))?;

    // 2. Compute cache key and check hit BEFORE reading the payload.
    let layered = footer.format_version >= 2 && !meta.layers.is_empty();
    let hash = if layered { cache_key_v2(&meta.layers) } else { footer.sha256_hex() };

    let base = cache_dir()?;
    fs::create_dir_all(&base)?;
    let cache_root = base.join(&hash);
    let rootfs = cache_root.join("rootfs");
    let ready_marker = cache_root.join(".ready");

    if ready_marker.exists() {
        // Warm path: cache exists, skip payload read entirely.
        if verbose {
            eprintln!("[xbin] warm start: cache hit {}", hash);
        }
        return exec_app(&meta, &rootfs);
    }

    // 3. Cold path: read payload + verify + extract.
    if verbose {
        eprintln!("[xbin] cold start: extracting {}", meta.name);
    }

    let payload = read_at(&mut exe, footer.payload_offset, footer.payload_csize as usize)?;

    // Verify Ed25519 signature (v3+ only).
    if footer.format_version >= 3 && footer.flags & format::FLAG_SIGNED != 0 {
        verify_ed25519(&footer, &mut exe, &payload, &meta_bytes)?;
        if verbose {
            eprintln!("[xbin] Ed25519 signature verified");
        }
    }

    // Verify SHA-256 integrity.
    if layered {
        let mut buf = payload.clone();
        buf.extend_from_slice(&meta_bytes);
        verify_sha256(&buf, &footer.payload_sha256)?;
    } else {
        verify_sha256(&payload, &footer.payload_sha256)?;
    }

    // Extract atomically.
    let lock = File::create(base.join(format!("{hash}.lock")))?;
    flock_exclusive(&lock)?;

    if !ready_marker.exists() {
        let blobs = slice_layers(&payload, footer.payload_offset, &meta, layered);
        extract_atomic(&blobs, &cache_root, &rootfs)?;
    }

    // 4. Exec into the extracted rootfs.
    if !meta.services.is_empty() {
        supervise_services(&meta, &rootfs)
    } else {
        exec_app(&meta, &rootfs)
    }
}

// ---------------------------------------------------------------------------
// Ed25519 signature verification
// ---------------------------------------------------------------------------

/// Verify Ed25519 signature: `Ed25519_verify(SHA256(payload‖meta), sig, public_key)`.
///
/// Trusted public keys are read from `~/.xbin/trusted-keys/` (or `$XBIN_TRUSTED_DIR`).
/// The launcher accepts the file if **any** trusted key verifies the signature.
fn verify_ed25519(footer: &Footer, exe: &mut File, payload: &[u8], meta_bytes: &[u8]) -> io::Result<()> {
    // Read signature block: [sig_size: u32le][signature: 64 bytes]
    let sig_data = read_at(exe, footer.sig_offset, 68)?;
    let sig_size = u32::from_le_bytes(sig_data[0..4].try_into().unwrap()) as usize;
    if sig_size != 64 {
        return Err(err("invalid Ed25519 signature size"));
    }
    let sig_bytes: &[u8; 64] = sig_data[4..68].try_into().unwrap();

    // Compute SHA-256(payload ‖ meta)
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(payload);
    hasher.update(meta_bytes);
    let hash = hasher.finalize();

    // Load trusted public keys from directory.
    let trusted_dir = trusted_keys_dir();
    if !trusted_dir.exists() {
        return Err(err(
            "trusted keys directory not found; cannot verify signature",
        ));
    }

    // Parse signature once.
    use ed25519_dalek::Signature;
    let sig = Signature::from_bytes(sig_bytes);

    use ed25519_dalek::VerifyingKey;

    let mut verified = false;
    let rd = fs::read_dir(&trusted_dir)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("reading trusted keys: {e}")))?;
    for entry in rd.flatten() {
        if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            let key_raw = match fs::read(entry.path()) {
                Ok(b) if b.len() == 32 => b,
                _ => continue,
            };
            let key_arr: [u8; 32] = match key_raw.try_into() {
                Ok(a) => a,
                Err(_) => continue,
            };
            let pub_key = match VerifyingKey::from_bytes(&key_arr) {
                Ok(k) => k,
                Err(_) => continue,
            };
            use ed25519_dalek::Verifier;
            if pub_key.verify(&hash, &sig).is_ok() {
                verified = true;
                break;
            }
        }
    }

    if !verified {
        return Err(err("Ed25519 signature verification failed"));
    }
    Ok(())
}

/// Return the directory where trusted Ed25519 public keys are stored.
/// Override via `$XBIN_TRUSTED_DIR`; default `~/.xbin/trusted-keys/`.
fn trusted_keys_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("XBIN_TRUSTED_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    PathBuf::from(home).join(".xbin").join("trusted-keys")
}

// ---------------------------------------------------------------------------
// Cache key (v2)
// ---------------------------------------------------------------------------

fn cache_key_v2(layers: &[Layer]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for l in layers {
        h.update(l.sha256.as_bytes());
    }
    let out = h.finalize();
    let mut s = String::with_capacity(64);
    for b in out {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn slice_layers<'a>(
    payload: &'a [u8],
    region_offset: u64,
    meta: &Metadata,
    layered: bool,
) -> Vec<&'a [u8]> {
    if !layered {
        return vec![payload];
    }
    meta.layers
        .iter()
        .map(|l| {
            let start = (l.offset - region_offset) as usize;
            let end = start + l.csize as usize;
            &payload[start..end]
        })
        .collect()
}

fn verify_sha256(data: &[u8], expected: &[u8; 32]) -> io::Result<()> {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    let got = h.finalize();
    if got.as_slice() != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "payload integrity check failed (SHA-256 mismatch)",
        ));
    }
    Ok(())
}

fn cache_dir() -> io::Result<PathBuf> {
    if let Some(d) = std::env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(d).join("xbin"));
    }
    let home = std::env::var_os("HOME")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME not set"))?;
    Ok(PathBuf::from(home).join(".cache").join("xbin"))
}

fn extract_atomic(blobs: &[&[u8]], cache_root: &Path, rootfs: &Path) -> io::Result<()> {
    let parent = cache_root.parent().unwrap_or(Path::new("/tmp"));
    fs::create_dir_all(parent)?;

    let tmp = parent.join(format!(".tmp-{}-{}", std::process::id(), nanos()));
    let tmp_rootfs = tmp.join("rootfs");
    fs::create_dir_all(&tmp_rootfs)?;

    for blob in blobs {
        let decoder = ruzstd::StreamingDecoder::new(io::Cursor::new(*blob))
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("zstd: {e}")))?;
        let mut archive = tar::Archive::new(decoder);
        archive.set_preserve_permissions(true);
        archive.set_overwrite(true);
        archive.unpack(&tmp_rootfs)?;
    }

    File::create(tmp.join(".ready"))?.write_all(b"1")?;

    match fs::rename(&tmp, cache_root) {
        Ok(()) => Ok(()),
        Err(_) if rootfs.exists() => {
            let _ = fs::remove_dir_all(&tmp);
            Ok(())
        }
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp);
            Err(e)
        }
    }
}

// ---------------------------------------------------------------------------
// Shared setup for exec paths (single-service + multi-service)
// ---------------------------------------------------------------------------

/// Enter user + mount namespace if isolation >= 2; no-op otherwise.
fn enter_namespace_if_needed(isolation: u8) -> io::Result<()> {
    if isolation >= 2 {
        enter_userns()?;
    }
    Ok(())
}

/// Build the process environment: host env + LD_LIBRARY_PATH + meta.env + ROOTFS substitution.
/// When `orig_cwd` is Some, inserts XBIN_ORIG_CWD (used by single-service exec).
fn setup_env(
    meta: &Metadata,
    rootfs: &Path,
    use_pivot: bool,
    orig_cwd: Option<&Path>,
) -> std::collections::BTreeMap<String, String> {
    let mut env: std::collections::BTreeMap<String, String> = std::env::vars().collect();

    if use_pivot {
        env.insert("LD_LIBRARY_PATH".into(),
                   LD_PATHS.join(":"));
    } else {
        let mut paths: Vec<String> = LD_PATHS.iter()
            .map(|p| rootfs.join(p))
            .filter(|p| p.exists())
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        if let Some(existing) = env.get("LD_LIBRARY_PATH") {
            if !existing.is_empty() {
                paths.push(existing.clone());
            }
        }
        env.insert("LD_LIBRARY_PATH".into(), paths.join(":"));
    }

    // PATH: bundled binaries (usr/bin, bin, usr/local/bin) before system PATH.
    if use_pivot {
        env.insert("PATH".into(), BIN_PATHS.join(":"));
    } else {
        let mut paths: Vec<String> = BIN_PATHS.iter()
            .map(|p| rootfs.join(p))
            .filter(|p| p.exists())
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        if let Some(existing) = env.get("PATH") {
            if !existing.is_empty() {
                paths.push(existing.clone());
            }
        }
        env.insert("PATH".into(), paths.join(":"));
    }

    if let Some(cwd) = orig_cwd {
        env.insert("XBIN_ORIG_CWD".into(), cwd.to_string_lossy().into_owned());
    }

    let rootfs_str = rootfs.to_string_lossy();
    for (k, v) in &meta.env {
        env.insert(k.clone(), v.replace("${ROOTFS}", &rootfs_str));
    }

    env
}

/// Resolve a rootfs path: absolute if using pivot_root, relative to rootfs otherwise.
fn make_resolve<'a>(rootfs: &'a Path, use_pivot: bool) -> impl Fn(&str) -> PathBuf + 'a {
    move |p: &str| -> PathBuf {
        if use_pivot {
            PathBuf::from(p)
        } else if let Some(stripped) = p.strip_prefix('/') {
            rootfs.join(stripped)
        } else {
            PathBuf::from(p)
        }
    }
}

/// Convert a BTreeMap<String,String> to a null-terminated Vec<CString> for execve.
fn env_to_cstrings(env: &std::collections::BTreeMap<String, String>) -> Vec<CString> {
    env.iter()
        .map(|(k, v)| cstr(format!("{k}={v}").as_bytes()))
        .collect()
}

// ---------------------------------------------------------------------------
// Single-service exec
// ---------------------------------------------------------------------------

fn exec_app(meta: &Metadata, rootfs: &Path) -> io::Result<()> {
    if meta.entrypoint.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "empty entrypoint"));
    }

    let orig_cwd = std::env::current_dir().ok();
    let use_pivot = meta.isolation >= 2;

    enter_namespace_if_needed(meta.isolation)?;
    if use_pivot {
        pivot_root_into(rootfs)?;
    }

    let resolve = make_resolve(rootfs, use_pivot);

    let prog = resolve(&meta.entrypoint[0]);
    let prog_c = cstr(prog.as_os_str().as_bytes());

    let mut argv: Vec<CString> = Vec::new();
    argv.push(prog_c.clone());
    for a in &meta.entrypoint[1..] {
        argv.push(cstr(resolve(a).as_os_str().as_bytes()));
    }
    for a in std::env::args_os().skip(1) {
        argv.push(cstr(a.as_bytes()));
    }

    let env = setup_env(meta, rootfs, use_pivot, orig_cwd.as_deref());
    let env_c = env_to_cstrings(&env);

    if let Some(cwd) = &meta.cwd {
        let dir = resolve(cwd);
        std::env::set_current_dir(&dir).ok();
    }

    let argv_ptrs = to_ptr_vec(&argv);
    let env_ptrs = to_ptr_vec(&env_c);
    // SAFETY: execve(2) replaces the current process. prog_c is a valid CString,
    // argv_ptrs and env_ptrs are null-terminated. We never return on success.
    unsafe {
        libc_execve(prog_c.as_ptr(), argv_ptrs.as_ptr(), env_ptrs.as_ptr());
    }
    Err(io::Error::last_os_error())
}

// ---------------------------------------------------------------------------
// Multi-process supervisor
// ---------------------------------------------------------------------------

/// Supervise multiple services: fork+exec each, health-check ports, wait for all.
fn supervise_services(meta: &Metadata, rootfs: &Path) -> io::Result<()> {
    let verbose = std::env::var_os("XBIN_VERBOSE").is_some();
    let use_pivot = meta.isolation >= 2;

    enter_namespace_if_needed(meta.isolation)?;
    if use_pivot {
        pivot_root_into(rootfs)?;
    }

    let base_env = setup_env(meta, rootfs, use_pivot, None);
    let resolve = make_resolve(rootfs, use_pivot);

    let children = fork_services(meta, &base_env, &resolve, rootfs, verbose)?;
    wait_for_health(meta, verbose)?;
    install_signal_handler(&children);
    wait_for_children(&children, verbose)
}

/// Fork+exec each service, returning (name, pid) pairs.
fn fork_services(
    meta: &Metadata,
    base_env: &std::collections::BTreeMap<String, String>,
    resolve: &dyn Fn(&str) -> PathBuf,
    rootfs: &Path,
    verbose: bool,
) -> io::Result<Vec<(String, i32)>> {
    let mut children = Vec::new();
    for svc in &meta.services {
        let prog = resolve(&svc.cmd[0]);
        let prog_c = cstr(prog.as_os_str().as_bytes());

        let mut argv: Vec<CString> = Vec::new();
        argv.push(prog_c.clone());
        for a in &svc.cmd[1..] {
            argv.push(cstr(resolve(a).as_os_str().as_bytes()));
        }

        let mut env = base_env.clone();
        for (k, v) in &svc.env {
            env.insert(k.clone(), v.replace("${ROOTFS}", &rootfs.to_string_lossy()));
        }
        let env_c = env_to_cstrings(&env);

        // SAFETY: fork(2) creates a copy of the calling process. The child
        // calls execve (which never returns on success) or exit(127).
        // The parent records the pid for waitpid tracking.
        unsafe {
            let pid = libc::fork();
            if pid < 0 {
                return Err(io::Error::last_os_error());
            }
            if pid == 0 {
                let argv_ptrs = to_ptr_vec(&argv);
                let env_ptrs = to_ptr_vec(&env_c);
                // SAFETY: execve(2) replaces the child process. All pointers
                // are valid CStrings, envp is null-terminated.
                libc_execve(prog_c.as_ptr(), argv_ptrs.as_ptr(), env_ptrs.as_ptr());
                eprintln!("[xbin] failed to exec {}: {}", svc.cmd[0], io::Error::last_os_error());
                std::process::exit(127);
            }
            if verbose {
                eprintln!("[xbin] service '{}' started (pid {})", svc.name, pid);
            }
            children.push((svc.name.clone(), pid));
        }
    }
    Ok(children)
}

/// Block until all services with ready_port are accepting connections.
fn wait_for_health(meta: &Metadata, verbose: bool) -> io::Result<()> {
    for svc in &meta.services {
        if svc.ready_port == 0 { continue; }
        let timeout = if svc.ready_timeout > 0 { svc.ready_timeout } else { 30 };
        if verbose {
            eprintln!("[xbin] waiting for {}:{} (timeout {}s)", svc.name, svc.ready_port, timeout);
        }
        wait_for_port(svc.ready_port, timeout)?;
        if verbose {
            eprintln!("[xbin] {}:{} is ready", svc.name, svc.ready_port);
        }
    }
    Ok(())
}

/// Wait for all children to exit. Forward SIGTERM/SIGINT to children.
/// Returns the exit code of the first failed service, or 0 if all succeeded.
fn wait_for_children(children: &[(String, i32)], verbose: bool) -> io::Result<()> {
    let mut exit_code = 0i32;
    let mut remaining = children.len();
    while remaining > 0 {
        let mut status: i32 = 0;
        // SAFETY: waitpid(2) with pid=-1 waits for any child. status is
        // filled by the kernel. We only read it after a successful return.
        let pid = unsafe { libc::waitpid(-1, &mut status, 0) };
        if pid < 0 { break; }
        remaining -= 1;

        if let Some((name, _)) = children.iter().find(|(_, p)| *p == pid) {
            if libc::WIFEXITED(status) {
                let code = libc::WEXITSTATUS(status);
                if verbose {
                    eprintln!("[xbin] service '{}' exited with code {}", name, code);
                }
                if code != 0 && exit_code == 0 { exit_code = code; }
            } else if libc::WIFSIGNALED(status) {
                let sig = libc::WTERMSIG(status);
                eprintln!("[xbin] service '{}' killed by signal {}", name, sig);
                if exit_code == 0 { exit_code = 128 + sig; }
                // One service died: kill the rest.
                for (_, cp) in children {
                    if *cp != pid {
                        // SAFETY: kill(2) sends a signal to a process we own
                        // (forked from us). SIGTERM is a graceful shutdown.
                        unsafe { libc::kill(*cp, libc::SIGTERM); }
                    }
                }
            }
        }
    }
    if exit_code != 0 { exit(exit_code); }
    Ok(())
}

fn wait_for_port(port: u16, timeout_secs: u64) -> io::Result<()> {
    use std::net::TcpStream;
    use std::time::{Duration, Instant};
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(_) => return Ok(()),
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("service port {port} not ready within {timeout_secs}s: {e}"),
            )),
        }
    }
}

fn install_signal_handler(children: &[(String, i32)]) {
    // SAFETY: We write to a static mut exactly once, before any signal handler
    // is installed. After install_signal_handler returns, CHILD_PIDS is only
    // read (never written) by signal_forward, so there is no data race.
    unsafe {
        CHILD_PIDS = children.iter().map(|(_, p)| *p).collect();
    }
    // SAFETY: signal(2) registers a C function pointer as a signal handler.
    // signal_forward only calls kill(2) (async-signal-safe) and reads CHILD_PIDS
    // (which is immutable after this point).
    unsafe {
        libc::signal(libc::SIGTERM, signal_forward as usize);
        libc::signal(libc::SIGINT, signal_forward as usize);
    }
}

static mut CHILD_PIDS: Vec<i32> = Vec::new();

extern "C" fn signal_forward(sig: i32) {
    // SAFETY: Called from a signal handler context. Only uses kill(2)
    // (async-signal-safe) and iterates CHILD_PIDS (immutable after install).
    unsafe {
        for &pid in &CHILD_PIDS {
            libc::kill(pid, sig);
        }
    }
}

/// Enter a new user + mount namespace (unprivileged).
fn enter_userns() -> io::Result<()> {
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

/// Switch root to `rootfs` via pivot_root(2).
/// The old root is mounted at `rootfs/.old_root` and immediately detached.
fn pivot_root_into(rootfs: &Path) -> io::Result<()> {
    let new_root = std::fs::canonicalize(rootfs)?;
    let new_root_c = cstr(new_root.as_os_str().as_bytes());

    // SAFETY: mount(2) bind-mounts rootfs onto itself. MS_BIND|MS_REC makes
    // it recursive. This is required for pivot_root(2) to accept rootfs as a
    // mount point. The mount point is immediately detached after pivot_root.
    unsafe {
        let rc = libc::mount(new_root_c.as_ptr(), new_root_c.as_ptr(),
                             std::ptr::null(), libc::MS_BIND | libc::MS_REC,
                             std::ptr::null());
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
    }

    let put_old = new_root.join(".old_root");
    std::fs::create_dir_all(&put_old)?;
    let put_old_c = cstr(put_old.as_os_str().as_bytes());

    let old_root_c = cstr(b"/.old_root");
    // SAFETY: pivot_root(2) (syscall 155 on x86_64) switches the root mount.
    // umount2(MNT_DETACH) lazily detaches the old root — files remain accessible
    // to existing file descriptors but are unreachable from the namespace.
    unsafe {
        let rc = libc::syscall(libc::SYS_pivot_root,
                               new_root_c.as_ptr(), put_old_c.as_ptr());
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        let rc = libc::umount2(old_root_c.as_ptr(), libc::MNT_DETACH);
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn write_proc(path: &str, contents: &str) -> io::Result<()> {
    let mut f = File::create(path)?;
    f.write_all(contents.as_bytes())?;
    Ok(())
}

fn cstr(bytes: &[u8]) -> CString {
    CString::new(bytes).unwrap_or_else(|_| CString::new("").unwrap())
}

fn to_ptr_vec(v: &[CString]) -> Vec<*const i8> {
    let mut ptrs: Vec<*const i8> = v.iter().map(|c| c.as_ptr()).collect();
    ptrs.push(std::ptr::null());
    ptrs
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

fn flock_exclusive(f: &File) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;
    const LOCK_EX: i32 = 2;
    // SAFETY: flock(2) acquires an exclusive lock on the file descriptor.
    // The fd is valid (from File::create). We hold the lock until `f` is dropped.
    let rc = unsafe { libc_flock(f.as_raw_fd(), LOCK_EX) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

extern "C" {
    #[link_name = "execve"]
    fn libc_execve(path: *const i8, argv: *const *const i8, envp: *const *const i8) -> i32;
    #[link_name = "flock"]
    fn libc_flock(fd: i32, operation: i32) -> i32;
}

fn err(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}
