//! xbin launcher stub.
//!
//! Embedded at the head of every .xbin file — this is the ELF the kernel runs.
//! Flow: open /proc/self/exe → read footer → verify integrity →
//! extract rootfs to ~/.cache/xbin/{sha256}/ (atomic) → exec the app.
//!
//! Level 0 isolation (MVP): LD_LIBRARY_PATH, no chroot. Levels 1/2
//! (chroot, user namespaces) in Phase 2 — see docs/src/roadmap.md.

mod format;

use format::Footer;
use serde::Deserialize;
use std::ffi::CString;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::OsStrExt;
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
    /// Payload layers (format v2). Empty for v1 (monolithic payload).
    #[serde(default)]
    layers: Vec<Layer>,
}

/// A v2 payload layer: an independent zstd(tar) blob, stacked during extraction
/// (later layers overwrite earlier ones — Docker-like layering model).
#[derive(Deserialize)]
struct Layer {
    #[serde(default)]
    kind: String,
    offset: u64,
    csize: u64,
    #[allow(dead_code)]
    usize: u64,
    /// SHA-256 (hex) of the compressed blob — used as stable per-layer cache key.
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

    // 1. Locate ourselves reliably (not argv[0] — caller-controlled).
    let mut exe = File::open("/proc/self/exe")?;
    let footer = Footer::read_from(&mut exe)?;

    // 2. Read JSON metadata.
    let meta_bytes = read_at(&mut exe, footer.meta_offset, footer.meta_size as usize)?;
    let meta: Metadata = serde_json::from_slice(&meta_bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("bad metadata: {e}")))?;

    // 3. Read the entire payload region (all layers, contiguous in v2).
    let payload = read_at(&mut exe, footer.payload_offset, footer.payload_csize as usize)?;

    // 4. Verify integrity.
    //    v1: SHA-256(payload).        v2: SHA-256(layers || metadata).
    let layered = footer.format_version >= 2 && !meta.layers.is_empty();
    if layered {
        let mut buf = payload.clone();
        buf.extend_from_slice(&meta_bytes);
        verify_sha256(&buf, &footer.payload_sha256)?;
    } else {
        verify_sha256(&payload, &footer.payload_sha256)?;
    }

    // 5. Cache key.
    //    v1: SHA-256 of the payload.  v2: SHA-256 of concatenated layer hashes
    //    (stable as long as layer content doesn't change — so an app-only rebuild
    //    keeps the runtime layer cached).
    let hash = if layered { cache_key_v2(&meta.layers) } else { footer.sha256_hex() };

    let base = cache_dir()?;
    fs::create_dir_all(&base)?;
    let cache_root = base.join(&hash);
    let rootfs = cache_root.join("rootfs");
    let ready_marker = cache_root.join(".ready");

    if !ready_marker.exists() {
        // Serialize concurrent instances: one extracts, others wait on the lock
        // then find the cache already ready. (Extraction is atomic via rename()
        // even without this lock; flock just avoids duplicated work.)
        let lock = File::create(base.join(format!("{hash}.lock")))?;
        flock_exclusive(&lock)?;

        if !ready_marker.exists() {
            if verbose {
                eprintln!("[xbin] cold start: extracting {}", meta.name);
            }
            // Split into layers (v2) or a single blob (v1).
            let blobs = slice_layers(&payload, footer.payload_offset, &meta, layered);
            extract_atomic(&blobs, &cache_root, &rootfs)?;
        }
        // Lock released when `lock` goes out of scope.
    } else if verbose {
        eprintln!("[xbin] warm start: cache hit {}", hash);
    }

    // 6. Build argv + env and exec into the extracted rootfs.
    exec_app(&meta, &rootfs)
}

/// v2 cache key: SHA-256 of the concatenation of each layer's hex hash.
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

/// Split the payload region into compressed blobs, in stacking order.
/// In v1, returns the entire payload as a single blob.
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

/// Read `len` bytes at absolute offset `off`.
fn read_at(f: &mut File, off: u64, len: usize) -> io::Result<Vec<u8>> {
    f.seek(SeekFrom::Start(off))?;
    let mut buf = vec![0u8; len];
    f.read_exact(&mut buf)?;
    Ok(buf)
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

/// Decompress one or more zstd(tar) layers into a unique temp directory
/// (stacked in order), then atomic rename() to the final cache location.
/// Prevents partial states (TOCTOU).
fn extract_atomic(blobs: &[&[u8]], cache_root: &Path, rootfs: &Path) -> io::Result<()> {
    let parent = cache_root.parent().unwrap_or(Path::new("/tmp"));
    fs::create_dir_all(parent)?;

    // Unique temp dir (pid + nanos) on the same filesystem as the target
    // (required for rename() to be atomic).
    let tmp = parent.join(format!(".tmp-{}-{}", std::process::id(), nanos()));
    let tmp_rootfs = tmp.join("rootfs");
    fs::create_dir_all(&tmp_rootfs)?;

    // Each layer: zstd → tar → unpack on top of previous layers.
    for blob in blobs {
        let decoder = ruzstd::StreamingDecoder::new(io::Cursor::new(*blob))
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("zstd: {e}")))?;
        let mut archive = tar::Archive::new(decoder);
        archive.set_preserve_permissions(true);
        archive.set_overwrite(true);
        archive.unpack(&tmp_rootfs)?;
    }

    // Completion marker.
    File::create(tmp.join(".ready"))?.write_all(b"1")?;

    // Atomic rename(). If another process won the race, discard our tmp.
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

/// Replace the current process with the embedded app.
fn exec_app(meta: &Metadata, rootfs: &Path) -> io::Result<()> {
    if meta.entrypoint.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "empty entrypoint"));
    }

    // Absolute entrypoint paths are relative to the extracted rootfs.
    let resolve = |p: &str| -> PathBuf {
        if let Some(stripped) = p.strip_prefix('/') {
            rootfs.join(stripped)
        } else {
            PathBuf::from(p)
        }
    };

    let prog = resolve(&meta.entrypoint[0]);
    let prog_c = cstr(prog.as_os_str().as_bytes());

    // argv: argv[0] = program path, then the rest of the entrypoint,
    // then any extra arguments passed by the user on the command line.
    let mut argv: Vec<CString> = Vec::new();
    argv.push(prog_c.clone());
    for a in &meta.entrypoint[1..] {
        argv.push(cstr(resolve(a).as_os_str().as_bytes()));
    }
    for a in std::env::args_os().skip(1) {
        argv.push(cstr(a.as_bytes()));
    }

    // env: inherit current environment, inject LD_LIBRARY_PATH into the rootfs
    // libs, then overlay the manifest's env entries.
    let mut env: std::collections::BTreeMap<String, String> = std::env::vars().collect();
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

    // Apply manifest env vars. The ${ROOTFS} token is replaced with the actual
    // rootfs path in the cache — known only at runtime. This lets the builder
    // declare paths (e.g. PYTHONPATH=${ROOTFS}/app/site-packages) without knowing
    // where the cache will be materialized.
    let rootfs_str = rootfs.to_string_lossy();
    for (k, v) in &meta.env {
        env.insert(k.clone(), v.replace("${ROOTFS}", &rootfs_str));
    }
    let env_c: Vec<CString> = env
        .iter()
        .map(|(k, v)| cstr(format!("{k}={v}").as_bytes()))
        .collect();

    // cwd
    if let Some(cwd) = &meta.cwd {
        let dir = resolve(cwd);
        std::env::set_current_dir(&dir).ok();
    }

    // execve: replaces the process. If it succeeds, we never return.
    let argv_ptrs = to_ptr_vec(&argv);
    let env_ptrs = to_ptr_vec(&env_c);
    unsafe {
        libc_execve(prog_c.as_ptr(), argv_ptrs.as_ptr(), env_ptrs.as_ptr());
    }
    // If we're here, execve failed.
    Err(io::Error::last_os_error())
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

/// Advisory exclusive lock on a file via flock(2). Blocks until acquired.
/// Released automatically when the file descriptor is closed.
fn flock_exclusive(f: &File) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;
    const LOCK_EX: i32 = 2;
    let rc = unsafe { libc_flock(f.as_raw_fd(), LOCK_EX) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

// Avoid the `nix` crate to keep the stub minimal: just the externs we need.
extern "C" {
    #[link_name = "execve"]
    fn libc_execve(path: *const i8, argv: *const *const i8, envp: *const *const i8) -> i32;
    #[link_name = "flock"]
    fn libc_flock(fd: i32, operation: i32) -> i32;
}
