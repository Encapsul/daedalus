//! xbin launcher stub.
//!
//! Embedded at the head of every .xbin file — this is the ELF the kernel runs.
//! Flow: open /proc/self/exe → read footer → verify integrity (sig → SHA-256) →
//! extract rootfs to ~/.cache/xbin/{sha256}/ (atomic) → exec the app.
//!
//! Isolation: level 0 = `LD_LIBRARY_PATH` (no sandbox), level 2 = user +
//! mount namespaces with `pivot_root` into the extracted rootfs and
//! optional seccomp BPF denylist. See `enter_namespace_if_needed()`,
//! `pivot_root_into()`, and `install_seccomp_denylist()`.

mod config;
mod squashfs_extract;

use serde::Deserialize;
use std::ffi::CString;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::exit;
use xbin_core::format::{self as format, read_at, Footer};

/// Standard library search paths for `LD_LIBRARY_PATH`.
/// Kept in sync with cli/xbin/build.py `LD_LIBRARY_PATH` construction.
#[cfg(target_arch = "x86_64")]
const LD_PATHS: &[&str] = &[
    "lib",
    "lib64",
    "usr/lib",
    "usr/lib64",
    "usr/lib/x86_64-linux-gnu",
];
#[cfg(target_arch = "aarch64")]
const LD_PATHS: &[&str] = &[
    "lib",
    "lib64",
    "usr/lib",
    "usr/lib64",
    "usr/lib/aarch64-linux-gnu",
];

/// Absolute forms of `LD_PATHS`, used after `pivot_root` where the process
/// root is the rootfs. `execvp` and the dynamic loader resolve relative PATH
/// / `LD_LIBRARY_PATH` entries against the current directory — with `cwd`
/// set to `/app` that misses `/usr/bin`, so pivot mode must use `/`-prefixed
/// entries (they resolve inside the new root).
const LD_PATHS_ABS: &[&str] = &[
    "/lib",
    "/lib64",
    "/usr/lib",
    "/usr/lib64",
    "/usr/lib/aarch64-linux-gnu",
];

/// Binary search paths for PATH, mirroring `LD_PATHS` for executables.
/// Bundled binaries (e.g. ffmpeg, gitleaks) land here via the rootfs.
const BIN_PATHS: &[&str] = &["usr/bin", "bin", "usr/local/bin"];

/// Absolute forms of `BIN_PATHS`; see `LD_PATHS_ABS`.
const BIN_PATHS_ABS: &[&str] = &["/usr/bin", "/bin", "/usr/local/bin"];

#[derive(Deserialize)]
struct Metadata {
    name: String,
    #[serde(default)]
    #[allow(dead_code)]
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
    seccomp: bool,
    #[serde(default)]
    services: Vec<Service>,
    #[serde(default)]
    crypto: Option<CryptoMeta>,
    #[serde(default)]
    payload_format: String,
    #[serde(default)]
    health_check: Option<HealthCheckMeta>,
}

#[derive(Deserialize)]
struct HealthCheckMeta {
    port: u16,
    #[serde(default = "default_health_endpoint")]
    endpoint: String,
    enabled: bool,
}

fn default_health_endpoint() -> String {
    "/health".to_string()
}

#[derive(Deserialize)]
struct CryptoMeta {
    nonce_hex: String,
    #[allow(dead_code)]
    tag_offset: usize,
    signing_seed_hex: String,
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
    #[allow(dead_code)]
    kind: String,
    offset: u64,
    csize: u64,
    #[serde(rename = "usize")]
    #[allow(dead_code)]
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

    // Load configuration (multi-layered: CLI args → local config → env vars → global config)
    let app_config = config::AppConfig::load();

    // 1. Read footer + metadata (small, fast).
    let mut exe = File::open("/proc/self/exe")?;
    let footer = Footer::read_from(&mut exe)?;
    let meta_bytes = read_at(&mut exe, footer.meta_offset, footer.meta_size as usize)?;
    let meta: Metadata = serde_json::from_slice(&meta_bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("bad metadata: {e}")))?;

    // 2. Compute cache key and check hit BEFORE reading the payload.
    let layered = footer.format_version >= 2 && !meta.layers.is_empty();
    let hash = if layered {
        cache_key_v2(&meta.layers)
    } else {
        footer.sha256_hex()
    };

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
        return exec_app(&meta, &rootfs, &app_config);
    }

    // 3. Cold path: read payload + verify + extract.
    if verbose {
        eprintln!("[xbin] cold start: extracting {}", meta.name);
    }

    let payload = read_at(
        &mut exe,
        footer.payload_offset,
        footer.payload_csize as usize,
    )?;

    // Verify Ed25519 signature (v3+ only).
    if footer.format_version >= 3 && footer.flags & format::FLAG_SIGNED != 0 {
        verify_ed25519(&footer, &mut exe, &payload, &meta_bytes)?;
        if verbose {
            eprintln!("[xbin] Ed25519 signature verified");
        }
    }

    // Verify SHA-256 integrity (hash = SHA-256(payload || meta_bytes)).
    let mut buf = payload.clone();
    buf.extend_from_slice(&meta_bytes);
    verify_sha256(&buf, &footer.payload_sha256)?;

    // Decrypt payload (v4+ with AES-256-GCM).
    // Happens AFTER signature + integrity verification — we only decrypt
    // what's already proven authentic.
    let payload = if footer.crypto_suite() == format::CRYPTO_AES_256_GCM {
        if let Some(ref crypto) = meta.crypto {
            if verbose {
                eprintln!("[xbin] decrypting AES-256-GCM payload");
            }
            decrypt_aes_gcm(&payload, crypto)?
        } else {
            return Err(err("encrypted payload but no crypto metadata"));
        }
    } else {
        payload
    };

    // Extract atomically.
    let lock = File::create(base.join(format!("{hash}.lock")))?;
    flock_exclusive(&lock)?;

    if !ready_marker.exists() {
        let is_squashfs = meta.payload_format == format::PAYLOAD_FORMAT_SQUASHFS;
        if is_squashfs {
            let blobs = slice_layers(&payload, footer.payload_offset, &meta, layered)?;
            extract_squashfs_atomic(&blobs, &cache_root, &rootfs)?;
        } else {
            let blobs = slice_layers(&payload, footer.payload_offset, &meta, layered)?;
            extract_atomic(&blobs, &cache_root, &rootfs)?;
        }
    }

    // 4. Exec into the extracted rootfs.
    if !meta.services.is_empty() {
        supervise_services(&meta, &rootfs, &app_config)
    } else {
        exec_app(&meta, &rootfs, &app_config)
    }
}

// ---------------------------------------------------------------------------
// Ed25519 signature verification
// ---------------------------------------------------------------------------

/// Verify Ed25519 signature: `Ed25519_verify(SHA256(payload‖meta), sig, public_key)`.
///
/// Trusted public keys are read from `~/.xbin/trusted-keys/` (or `$XBIN_TRUSTED_DIR`).
/// The launcher accepts the file if **any** trusted key verifies the signature.
fn verify_ed25519(
    footer: &Footer,
    exe: &mut File,
    payload: &[u8],
    meta_bytes: &[u8],
) -> io::Result<()> {
    // Read signature block: [sig_size: u32le][signature: 64 bytes]
    let sig_data = read_at(exe, footer.sig_offset, 68)?;
    let size_bytes: [u8; 4] = sig_data[0..4]
        .try_into()
        .map_err(|_| err("signature block too small"))?;
    let sig_size = u32::from_le_bytes(size_bytes) as usize;
    if sig_size != 64 {
        return Err(err("invalid Ed25519 signature size"));
    }
    let sig_bytes: &[u8; 64] = sig_data[4..68]
        .try_into()
        .map_err(|_| err("invalid signature block size"))?;

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
    hex::encode(h.finalize())
}

fn slice_layers<'a>(
    payload: &'a [u8],
    region_offset: u64,
    meta: &Metadata,
    layered: bool,
) -> io::Result<Vec<&'a [u8]>> {
    if !layered {
        return Ok(vec![payload]);
    }
    meta.layers
        .iter()
        .map(|l| {
            let start = (l.offset - region_offset) as usize;
            let end = start
                .checked_add(l.csize as usize)
                .ok_or_else(|| err("layer size overflow"))?;
            payload
                .get(start..end)
                .ok_or_else(|| err("layer extends beyond payload boundary"))
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

// ---------------------------------------------------------------------------
// AES-256-GCM decryption
// ---------------------------------------------------------------------------

/// Derive a 32-byte AES key from an Ed25519 signing seed via HKDF-SHA256.
/// Uses the shared implementation in xbin-core.
fn hkdf_derive_key(signing_seed: &[u8]) -> io::Result<[u8; 32]> {
    let seed: &[u8; 32] = signing_seed
        .try_into()
        .map_err(|_| err("signing seed must be exactly 32 bytes"))?;
    xbin_core::encrypt::hkdf_derive_key(seed)
        .map_err(|e| err(&format!("HKDF key derivation failed: {e}")))
}

/// Decrypt an AES-256-GCM payload.
///
/// The signing seed is stored in metadata (protected by Ed25519 signature).
/// We derive the AES key from it via HKDF, then decrypt.
///
/// Ciphertext layout: [plaintext bytes][16-byte GCM tag]
fn decrypt_aes_gcm(ciphertext: &[u8], crypto: &CryptoMeta) -> io::Result<Vec<u8>> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};

    let signing_seed = hex_decode(&crypto.signing_seed_hex)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad signing seed hex"))?;
    if signing_seed.len() != 32 {
        return Err(err("signing seed must be 32 bytes"));
    }

    let aes_key = hkdf_derive_key(&signing_seed)?;
    let cipher = Aes256Gcm::new_from_slice(&aes_key)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("AES init: {e}")))?;

    let nonce_bytes = hex_decode(&crypto.nonce_hex)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad nonce hex"))?;
    let nonce = Nonce::from_slice(&nonce_bytes);

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("AES decrypt: {e}")))
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
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
    atomic_extract(cache_root, rootfs, |tmp_rootfs| {
        for blob in blobs {
            let decoder = ruzstd::StreamingDecoder::new(io::Cursor::new(*blob))
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("zstd: {e}")))?;
            let mut archive = tar::Archive::new(decoder);
            archive.set_preserve_permissions(true);
            archive.set_overwrite(true);
            archive.unpack(tmp_rootfs)?;
        }
        Ok(())
    })
}

fn extract_squashfs_atomic(blobs: &[&[u8]], cache_root: &Path, rootfs: &Path) -> io::Result<()> {
    atomic_extract(cache_root, rootfs, |tmp_rootfs| {
        squashfs_extract::extract_squashfs_layers(blobs, tmp_rootfs)
    })
}

/// Shared atomic extraction: create tmp dir, run extraction closure, write .ready, rename.
fn atomic_extract(
    cache_root: &Path,
    rootfs: &Path,
    extract_fn: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<()> {
    let parent = cache_root.parent().unwrap_or(Path::new("/tmp"));
    fs::create_dir_all(parent)?;

    let tmp = parent.join(format!(".tmp-{}-{}", std::process::id(), nanos()));
    let tmp_rootfs = tmp.join("rootfs");
    fs::create_dir_all(&tmp_rootfs)?;

    extract_fn(&tmp_rootfs)?;

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

/// Build the process environment: host env + `LD_LIBRARY_PATH` + meta.env + `ROOTFS` substitution.
/// When `orig_cwd` is Some, inserts `XBIN_ORIG_CWD` (used by single-service exec).
fn setup_env(
    meta: &Metadata,
    rootfs: &Path,
    use_pivot: bool,
    orig_cwd: Option<&Path>,
    app_config: &config::AppConfig,
) -> io::Result<std::collections::BTreeMap<String, String>> {
    let mut env: std::collections::BTreeMap<String, String> = std::env::vars().collect();

    if use_pivot {
        env.insert("LD_LIBRARY_PATH".into(), LD_PATHS_ABS.join(":"));
    } else {
        let mut paths: Vec<String> = LD_PATHS
            .iter()
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
        env.insert("PATH".into(), BIN_PATHS_ABS.join(":"));
    } else {
        let mut paths: Vec<String> = BIN_PATHS
            .iter()
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

    // Merge secrets from config file
    if let Some(secrets) = &app_config.secrets {
        for (k, v) in secrets {
            env.insert(format!("XBIN_SECRET_{}", k.to_uppercase()), v.clone());
        }
    }

    // Merge database URL from config
    if let Some(url) = app_config.get_database_url() {
        env.insert("DATABASE_URL".into(), url);
    }

    Ok(env)
}

/// Resolve a rootfs path: absolute if using `pivot_root`, relative to rootfs otherwise.
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

/// Convert a `BTreeMap<String,String>` to a null-terminated `Vec<CString>` for execve.
fn env_to_cstrings(env: &std::collections::BTreeMap<String, String>) -> io::Result<Vec<CString>> {
    env.iter()
        .map(|(k, v)| cstr(format!("{k}={v}").as_bytes()))
        .collect()
}

/// Check if an executable path exists and is executable.
/// Searches PATH directories when given a bare name (no `/`).
fn is_executable(prog: &[u8]) -> bool {
    if prog.is_empty() {
        return false;
    }
    let path = String::from_utf8_lossy(prog);
    // If it's an absolute or relative path with a directory component, check it directly.
    if path.contains('/') {
        return check_executable(&path);
    }
    // Otherwise search PATH directories (mirrors execvp behavior).
    let paths = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(path.as_ref());
        if check_executable(&candidate.to_string_lossy()) {
            return true;
        }
    }
    false
}

/// Check if a specific path points to an executable file.
fn check_executable(path: &str) -> bool {
    std::fs::metadata(path).is_ok_and(|m| {
        m.is_file() && {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                m.permissions().mode() & 0o111 != 0
            }
            #[cfg(windows)]
            {
                true
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Single-service exec
// ---------------------------------------------------------------------------

fn exec_app(meta: &Metadata, rootfs: &Path, app_config: &config::AppConfig) -> io::Result<()> {
    if meta.entrypoint.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "empty entrypoint",
        ));
    }

    let orig_cwd = std::env::current_dir().ok();
    let use_pivot = meta.isolation >= 2;

    maybe_start_health(meta);

    enter_namespace_if_needed(meta.isolation)?;
    if use_pivot {
        pivot_root_into(rootfs)?;
        if meta.seccomp {
            if let Err(e) = install_seccomp_denylist() {
                eprintln!(
                    "[xbin] warning: seccomp not available, running without syscall filter: {e}"
                );
            }
        }
    }

    let resolve = make_resolve(rootfs, use_pivot);

    let prog = resolve(&meta.entrypoint[0]);
    let prog_c = cstr(prog.as_os_str().as_bytes())?;

    let prog_path_bytes = prog.as_os_str().as_bytes();
    let _prog_path_str = std::str::from_utf8(prog_path_bytes).unwrap_or_default();

    // Compiled binaries (go/binary) exec `entrypoint[0]` directly; interpreted
    // runtimes get their interpreter prepended to argv.
    let direct_exec = matches!(meta.runtime.as_str(), "go" | "binary");
    let interpreter_name = match meta.runtime.as_str() {
        "php" => "php",
        "python" => "python3",
        "node" => "node",
        "ruby" => "ruby",
        "perl" => "perl",
        "java" => "java",
        "deno" => "deno",
        _ => "bash",
    };

    let mut argv: Vec<CString> = Vec::new();
    if direct_exec {
        if !is_executable(prog_path_bytes) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("[xbin] error: executable '{}' not found", prog.display()),
            ));
        }
        argv.push(prog_c.clone());
        for a in &meta.entrypoint[1..] {
            argv.push(cstr(resolve(a).as_os_str().as_bytes())?);
        }
    } else {
        if !is_executable(interpreter_name.as_bytes()) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("[xbin] error: interpreter '{}' not found", interpreter_name),
            ));
        }
        argv.push(cstr(interpreter_name.as_bytes())?);
        for a in &meta.entrypoint[1..] {
            argv.push(cstr(resolve(a).as_os_str().as_bytes())?);
        }
    }
    for a in std::env::args_os().skip(1) {
        argv.push(cstr(a.as_bytes())?);
    }

    let env = setup_env(meta, rootfs, use_pivot, orig_cwd.as_deref(), app_config)?;

    if let Some(cwd) = &meta.cwd {
        let dir = resolve(cwd);
        std::env::set_current_dir(&dir).ok();
    }

    // Set environment variables so execvp inherits them.
    for (k, v) in &env {
        std::env::set_var(k, v);
    }

    let argv_ptrs = to_ptr_vec(&argv);
    // SAFETY: execvp(3) replaces the current process. prog_c is a valid CString,
    // argv_ptrs is null-terminated. We never return on success.
    // execvp searches PATH for bare command names (e.g. "python3") and uses
    // absolute paths as-is (e.g. "/app/app.py"). Environment is inherited
    // from the current process after set_var calls above.
    unsafe {
        libc_execvp(prog_c.as_ptr(), argv_ptrs.as_ptr());
    }
    Err(io::Error::last_os_error())
}

// ---------------------------------------------------------------------------
// Multi-process supervisor
// ---------------------------------------------------------------------------

/// Supervise multiple services: fork+exec each, health-check ports, wait for all.
fn supervise_services(
    meta: &Metadata,
    rootfs: &Path,
    app_config: &config::AppConfig,
) -> io::Result<()> {
    let verbose = std::env::var_os("XBIN_VERBOSE").is_some();
    let use_pivot = meta.isolation >= 2;

    maybe_start_health(meta);

    enter_namespace_if_needed(meta.isolation)?;
    if use_pivot {
        pivot_root_into(rootfs)?;
        if meta.seccomp {
            if let Err(e) = install_seccomp_denylist() {
                eprintln!(
                    "[xbin] warning: seccomp not available, running without syscall filter: {e}"
                );
            }
        }
    }

    let base_env = setup_env(meta, rootfs, use_pivot, None, app_config)?;
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
        let prog_c = cstr(prog.as_os_str().as_bytes())?;

        let mut argv: Vec<CString> = Vec::new();
        argv.push(prog_c.clone());
        for a in &svc.cmd[1..] {
            argv.push(cstr(resolve(a).as_os_str().as_bytes())?);
        }

        let mut env = base_env.clone();
        for (k, v) in &svc.env {
            env.insert(k.clone(), v.replace("${ROOTFS}", &rootfs.to_string_lossy()));
        }
        let env_c = env_to_cstrings(&env)?;

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
                eprintln!(
                    "[xbin] failed to exec {}: {}",
                    svc.cmd[0],
                    io::Error::last_os_error()
                );
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

/// Block until all services with `ready_port` are accepting connections.
fn wait_for_health(meta: &Metadata, verbose: bool) -> io::Result<()> {
    for svc in &meta.services {
        if svc.ready_port == 0 {
            continue;
        }
        let timeout = if svc.ready_timeout > 0 {
            svc.ready_timeout
        } else {
            30
        };
        if verbose {
            eprintln!(
                "[xbin] waiting for {}:{} (timeout {}s)",
                svc.name, svc.ready_port, timeout
            );
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
        if pid < 0 {
            break;
        }
        remaining -= 1;

        if let Some((name, _)) = children.iter().find(|(_, p)| *p == pid) {
            if libc::WIFEXITED(status) {
                let code = libc::WEXITSTATUS(status);
                if verbose {
                    eprintln!("[xbin] service '{}' exited with code {}", name, code);
                }
                if code != 0 && exit_code == 0 {
                    exit_code = code;
                }
            } else if libc::WIFSIGNALED(status) {
                let sig = libc::WTERMSIG(status);
                eprintln!("[xbin] service '{}' killed by signal {}", name, sig);
                if exit_code == 0 {
                    exit_code = 128 + sig;
                }
                // One service died: kill the rest.
                for (_, cp) in children {
                    if *cp != pid {
                        // SAFETY: kill(2) sends a signal to a process we own
                        // (forked from us). SIGTERM is a graceful shutdown.
                        unsafe {
                            libc::kill(*cp, libc::SIGTERM);
                        }
                    }
                }
            }
        }
    }
    if exit_code != 0 {
        exit(exit_code);
    }
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
            Err(e) => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("service port {port} not ready within {timeout_secs}s: {e}"),
                ))
            }
        }
    }
}

fn install_signal_handler(children: &[(String, i32)]) {
    use std::sync::atomic::{compiler_fence, Ordering};
    // SAFETY: We write to a static mut exactly once, before any signal handler
    // is installed. After install_signal_handler returns, CHILD_PIDS is only
    // read (never written) by signal_forward, so there is no data race.
    unsafe {
        CHILD_PIDS = children.iter().map(|(_, p)| *p).collect();
    }
    // Prevent compiler from reordering the write past the signal() registration.
    // On x86_64, TSO ensures store visibility. On aarch64, the kernel signal()
    // syscall provides the necessary memory barriers.
    compiler_fence(Ordering::Release);
    // SAFETY: signal(2) registers a C function pointer as a signal handler.
    // signal_forward only calls kill(2) (async-signal-safe) and reads CHILD_PIDS
    // (which is immutable after this point).
    unsafe {
        libc::signal(libc::SIGTERM, signal_forward as *const () as usize);
        libc::signal(libc::SIGINT, signal_forward as *const () as usize);
    }
}

static mut CHILD_PIDS: Vec<i32> = Vec::new();

extern "C" fn signal_forward(sig: i32) {
    // SAFETY: Called from a signal handler context. Only uses kill(2)
    // (async-signal-safe) and iterates CHILD_PIDS (immutable after install).
    // We use `&raw const` to avoid creating a shared reference to a mutable static.
    unsafe {
        let pids: *const Vec<i32> = &raw const CHILD_PIDS;
        for &pid in &*pids {
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

// ---------------------------------------------------------------------------
// Seccomp BPF denylist
// ---------------------------------------------------------------------------

/// Install a seccomp-bpf denylist after `pivot_root`.
///
/// Blocks syscalls that have no legitimate use in a packaged web/server app
/// and represent escalation paths not covered by namespace isolation.
/// The list is conservative: only ~14 syscalls, all clearly dangerous.
/// Apps that work without seccomp continue working with it.
fn install_seccomp_denylist() -> io::Result<()> {
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

    // Audit arch + syscall numbers — differ between x86_64 and aarch64.
    #[cfg(target_arch = "x86_64")]
    const AUDIT_ARCH: u32 = 0xC000_003E;
    #[cfg(target_arch = "x86_64")]
    const SYS_PTRACE: u32 = 101;
    #[cfg(target_arch = "x86_64")]
    const SYS_MOUNT: u32 = 165;
    #[cfg(target_arch = "x86_64")]
    const SYS_UMOUNT2: u32 = 166;
    #[cfg(target_arch = "x86_64")]
    const SYS_PIVOT_ROOT: u32 = 155;
    #[cfg(target_arch = "x86_64")]
    const SYS_REBOOT: u32 = 169;
    #[cfg(target_arch = "x86_64")]
    const SYS_SETHOSTNAME: u32 = 170;
    #[cfg(target_arch = "x86_64")]
    const SYS_SETDOMAINNAME: u32 = 171;
    #[cfg(target_arch = "x86_64")]
    const SYS_SWAPON: u32 = 175;
    #[cfg(target_arch = "x86_64")]
    const SYS_SWAPOFF: u32 = 176;
    #[cfg(target_arch = "x86_64")]
    const SYS_ACCT: u32 = 163;
    #[cfg(target_arch = "x86_64")]
    const SYS_KEXEC_LOAD: u32 = 246;
    #[cfg(target_arch = "x86_64")]
    const SYS_INIT_MODULE: u32 = 175;
    #[cfg(target_arch = "x86_64")]
    const SYS_FINIT_MODULE: u32 = 313;
    #[cfg(target_arch = "x86_64")]
    const SYS_DELETE_MODULE: u32 = 176;
    #[cfg(target_arch = "x86_64")]
    const SYS_NFSSERVCTL: u32 = 423;
    #[cfg(target_arch = "x86_64")]
    const SYS_KEXEC_FILE_LOAD: u32 = 320;

    #[cfg(target_arch = "aarch64")]
    const AUDIT_ARCH: u32 = 0xC000_00B7;
    #[cfg(target_arch = "aarch64")]
    const SYS_PTRACE: u32 = 117;
    #[cfg(target_arch = "aarch64")]
    const SYS_MOUNT: u32 = 40;
    #[cfg(target_arch = "aarch64")]
    const SYS_UMOUNT2: u32 = 39;
    #[cfg(target_arch = "aarch64")]
    const SYS_PIVOT_ROOT: u32 = 41;
    #[cfg(target_arch = "aarch64")]
    const SYS_REBOOT: u32 = 142;
    #[cfg(target_arch = "aarch64")]
    const SYS_SETHOSTNAME: u32 = 160;
    #[cfg(target_arch = "aarch64")]
    const SYS_SETDOMAINNAME: u32 = 161;
    #[cfg(target_arch = "aarch64")]
    const SYS_SWAPON: u32 = 233;
    #[cfg(target_arch = "aarch64")]
    const SYS_SWAPOFF: u32 = 234;
    #[cfg(target_arch = "aarch64")]
    const SYS_ACCT: u32 = 89;
    // kexec_load does not exist on aarch64 (generic syscall table excludes it).
    // On x86_64, kexec_load is syscall 246. We keep the constant for both
    // architectures to avoid conditional compilation in the shared filter.
    #[cfg(target_arch = "aarch64")]
    const SYS_KEXEC_LOAD: u32 = 106;
    #[cfg(target_arch = "aarch64")]
    const SYS_INIT_MODULE: u32 = 105;
    #[cfg(target_arch = "aarch64")]
    const SYS_FINIT_MODULE: u32 = 278;
    #[cfg(target_arch = "aarch64")]
    const SYS_DELETE_MODULE: u32 = 106;
    #[cfg(target_arch = "aarch64")]
    const SYS_NFSSERVCTL: u32 = 26;
    #[cfg(target_arch = "aarch64")]
    const SYS_KEXEC_FILE_LOAD: u32 = 320;

    // Helper: load seccomp_data.arch (offset 4, 4 bytes).
    let arch_load = |code: u16, jt: u8, jf: u8, k: u32| libc::sock_filter {
        code: code | BPF_W | BPF_ABS,
        jt,
        jf,
        k,
    };
    // Helper: BPF_JMP | BPF_JEQ.
    let jmp_eq = |k: u32, jt: u8, jf: u8| libc::sock_filter {
        code: BPF_JMP | BPF_JEQ | BPF_K,
        jt,
        jf,
        k,
    };
    // Helper: BPF_RET.
    let ret = |k: u32| libc::sock_filter {
        code: BPF_RET | BPF_K,
        jt: 0,
        jf: 0,
        k,
    };

    // BPF jt/jf are forward skip counts from the *next* instruction.
    // Instruction layout:
    //   [0]  arch_load(4)        — load seccomp_data.arch
    //   [1]  jmp_eq AUDIT_ARCH   — if wrong arch → ALLOW at [20]
    //   [2]  arch_load(0)        — load seccomp_data.nr
    //   [3]  jmp_eq SYS_PTRACE       → KILL at [19]  (skip 15)
    //   [4]  jmp_eq SYS_MOUNT        → KILL at [19]  (skip 14)
    //   [5]  jmp_eq SYS_UMOUNT2      → KILL at [19]  (skip 13)
    //   [6]  jmp_eq SYS_PIVOT_ROOT   → KILL at [19]  (skip 12)
    //   [7]  jmp_eq SYS_REBOOT       → KILL at [19]  (skip 11)
    //   [8]  jmp_eq SYS_SETHOSTNAME  → KILL at [19]  (skip 10)
    //   [9]  jmp_eq SYS_SETDOMAINNAME→ KILL at [19]  (skip 9)
    //   [10] jmp_eq SYS_SWAPON       → KILL at [19]  (skip 8)
    //   [11] jmp_eq SYS_SWAPOFF      → KILL at [19]  (skip 7)
    //   [12] jmp_eq SYS_ACCT         → KILL at [19]  (skip 6)
    //   [13] jmp_eq SYS_NFSSERVCTL   → KILL at [19]  (skip 5)
    //   [14] jmp_eq SYS_KEXEC_LOAD   → KILL at [19]  (skip 4)
    //   [15] jmp_eq SYS_INIT_MODULE  → KILL at [19]  (skip 3)
    //   [16] jmp_eq SYS_FINIT_MODULE → KILL at [19]  (skip 2)
    //   [17] jmp_eq SYS_DELETE_MODULE→ KILL at [19]  (skip 1)
    //   [18] jmp_eq SYS_KEXEC_FILE_LOAD → KILL at [19] (skip 0, fall through)
    //   [19] ret SECCOMP_RET_KILL_PROCESS
    //   [20] ret SECCOMP_RET_ALLOW
    //
    // NOTE: On x86_64, SYS_SWAPON(175) == SYS_INIT_MODULE(175) and
    // SYS_SWAPOFF(176) == SYS_DELETE_MODULE(176) — kernel ABI duplicates.
    // On aarch64, SYS_KEXEC_LOAD(106) == SYS_DELETE_MODULE(106) — kexec_load
    // does not exist on aarch64, so the value collides with delete_module.
    // All duplicates are harmless (both entries KILL), kept for clarity.
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

/// Switch root to `rootfs` via `pivot_root(2)`.
/// The old root is mounted at `rootfs/.old_root` and immediately detached.
fn pivot_root_into(rootfs: &Path) -> io::Result<()> {
    let new_root = std::fs::canonicalize(rootfs)?;
    let new_root_c = cstr(new_root.as_os_str().as_bytes())?;

    // SAFETY: mount(2) bind-mounts rootfs onto itself. MS_BIND|MS_REC makes
    // it recursive. This is required for pivot_root(2) to accept rootfs as a
    // mount point. The mount point is immediately detached after pivot_root.
    unsafe {
        let rc = libc::mount(
            new_root_c.as_ptr(),
            new_root_c.as_ptr(),
            std::ptr::null(),
            libc::MS_BIND | libc::MS_REC,
            std::ptr::null(),
        );
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
    }

    let put_old = new_root.join(".old_root");
    std::fs::create_dir_all(&put_old)?;
    let put_old_c = cstr(put_old.as_os_str().as_bytes())?;

    let old_root_c = cstr(b"/.old_root")?;
    // SAFETY: pivot_root(2) (syscall 155 on x86_64) switches the root mount.
    // umount2(MNT_DETACH) lazily detaches the old root — files remain accessible
    // to existing file descriptors but are unreachable from the namespace.
    unsafe {
        let rc = libc::syscall(
            libc::SYS_pivot_root,
            new_root_c.as_ptr(),
            put_old_c.as_ptr(),
        );
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

fn cstr(bytes: &[u8]) -> io::Result<CString> {
    CString::new(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "path contains null byte"))
}

fn to_ptr_vec(v: &[CString]) -> Vec<*const core::ffi::c_char> {
    let mut ptrs: Vec<*const core::ffi::c_char> = v.iter().map(|c| c.as_ptr()).collect();
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
    #[link_name = "execvp"]
    fn libc_execvp(path: *const core::ffi::c_char, argv: *const *const core::ffi::c_char) -> i32;
    #[link_name = "execve"]
    fn libc_execve(
        path: *const core::ffi::c_char,
        argv: *const *const core::ffi::c_char,
        envp: *const *const core::ffi::c_char,
    ) -> i32;
    #[link_name = "flock"]
    fn libc_flock(fd: i32, operation: i32) -> i32;
}

fn err(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

// ---------------------------------------------------------------------------
// Health check HTTP server
// ---------------------------------------------------------------------------

/// Spawn a daemon TCP thread that responds to health check requests.
/// Runs until the process exits (thread is detached).
fn spawn_health_server(port: u16, endpoint: String) {
    let listener = match std::net::TcpListener::bind(format!("127.0.0.1:{port}")) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[xbin] warning: health check server failed to bind on port {port}: {e}");
            return;
        }
    };
    let _ = listener.set_nonblocking(true);

    std::thread::spawn(move || {
        let response_200 = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 14\r\nConnection: close\r\n\r\n{\"status\":\"ok\"}";
        let response_404 =
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buf = [0u8; 1024];
                    let n = stream.read(&mut buf).unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..n]);
                    let first_line = request.lines().next().unwrap_or("");
                    if first_line.starts_with("GET") && first_line.contains(endpoint.as_str()) {
                        let _ = stream.write_all(response_200);
                    } else {
                        let _ = stream.write_all(response_404);
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }
    });
}

/// Start the health check server if configured in metadata.
fn maybe_start_health(meta: &Metadata) {
    if let Some(ref hc) = meta.health_check {
        if hc.enabled && hc.port > 0 {
            spawn_health_server(hc.port, hc.endpoint.clone());
        }
    }
}
