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

#[derive(Deserialize)]
struct Metadata {
    name: String,
    #[serde(default)]
    runtime: String,
    entrypoint: Vec<String>,
    #[serde(default)]
    env: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    layers: Vec<Layer>,
    #[serde(default)]
    isolation: u8,
}

#[derive(Deserialize)]
struct Layer {
    #[serde(default)]
    kind: String,
    offset: u64,
    csize: u64,
    #[allow(dead_code)]
    usize: u64,
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
    exec_app(&meta, &rootfs)
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

fn exec_app(meta: &Metadata, rootfs: &Path) -> io::Result<()> {
    if meta.entrypoint.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "empty entrypoint"));
    }

    // Capture the original CWD BEFORE pivot_root changes the root.
    let orig_cwd = std::env::current_dir().ok();

    let use_pivot = meta.isolation >= 2;

    if use_pivot {
        enter_userns()?;
        pivot_root_into(rootfs)?;
    }

    let resolve = |p: &str| -> PathBuf {
        if use_pivot {
            PathBuf::from(p)
        } else if let Some(stripped) = p.strip_prefix('/') {
            rootfs.join(stripped)
        } else {
            PathBuf::from(p)
        }
    };

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

    let mut env: std::collections::BTreeMap<String, String> = std::env::vars().collect();
    if use_pivot {
        env.insert("LD_LIBRARY_PATH".into(),
                   "/lib:/lib64:/usr/lib:/usr/lib64:/usr/lib/x86_64-linux-gnu".into());
    } else {
        let lib_dirs = [
            rootfs.join("lib"),
            rootfs.join("lib64"),
            rootfs.join("usr/lib"),
            rootfs.join("usr/lib64"),
            rootfs.join("usr/lib/x86_64-linux-gnu"),
        ];
        let mut ld = lib_dirs
            .iter()
            .filter(|p| p.exists())
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(":");
        if let Some(existing) = env.get("LD_LIBRARY_PATH") {
            if !existing.is_empty() {
                ld.push(':');
                ld.push_str(existing);
            }
        }
        env.insert("LD_LIBRARY_PATH".into(), ld);
    }

    // XBIN_ORIG_CWD was captured before pivot_root above.
    if let Some(cwd) = orig_cwd.as_ref() {
        env.insert("XBIN_ORIG_CWD".into(), cwd.to_string_lossy().into_owned());
    }

    let rootfs_str = rootfs.to_string_lossy();
    for (k, v) in &meta.env {
        env.insert(k.clone(), v.replace("${ROOTFS}", &rootfs_str));
    }
    let env_c: Vec<CString> = env
        .iter()
        .map(|(k, v)| cstr(format!("{k}={v}").as_bytes()))
        .collect();

    if let Some(cwd) = &meta.cwd {
        let dir = resolve(cwd);
        std::env::set_current_dir(&dir).ok();
    }

    let argv_ptrs = to_ptr_vec(&argv);
    let env_ptrs = to_ptr_vec(&env_c);
    unsafe {
        libc_execve(prog_c.as_ptr(), argv_ptrs.as_ptr(), env_ptrs.as_ptr());
    }
    Err(io::Error::last_os_error())
}

/// Enter a new user + mount namespace (unprivileged).
fn enter_userns() -> io::Result<()> {
    let uid = unsafe { libc::getuid() };
    let gid = unsafe { libc::getgid() };

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

    // Bind-mount rootfs onto itself so pivot_root(2) accepts it as a mount point.
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
    unsafe {
        // SYS_pivot_root = 155 on x86_64
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
