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
mod crypto;
mod exec;
mod extraction;
mod health_gate;
#[cfg(target_os = "linux")]
mod landlock;
#[cfg(target_os = "macos")]
mod macos_sandbox;
#[cfg(target_os = "linux")]
mod namespace;
mod seccomp;
mod squashfs_extract;
mod update_url;
#[cfg(target_os = "windows")]
mod win;

use serde::Deserialize;
#[cfg(unix)]
use std::ffi::CString;
use std::fs::{self, File};
use std::io::{self};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::exit;
use xbin_core::format::{self as format, read_at, Footer};
use xbin_core::sisr::health::{HealthCheckPolicy, HealthState, HealthStore};
use xbin_core::sisr::resilience::{backup_path_for, create_backup, discard_backup, restore_backup};

/// Standard library search paths for `LD_LIBRARY_PATH`.
/// Kept in sync with cli/xbin/build.py `LD_LIBRARY_PATH` construction.
/// Linux-only: Windows loads DLLs from the exe dir / PATH / System32.
#[cfg(all(unix, target_arch = "x86_64"))]
const LD_PATHS: &[&str] = &[
    "lib",
    "lib64",
    "usr/lib",
    "usr/lib64",
    "usr/lib/x86_64-linux-gnu",
];
#[cfg(all(unix, target_arch = "aarch64"))]
const LD_PATHS: &[&str] = &[
    "lib",
    "lib64",
    "usr/lib",
    "usr/lib64",
    "usr/lib/aarch64-linux-gnu",
];
#[cfg(all(unix, target_arch = "x86"))]
const LD_PATHS: &[&str] = &["lib", "usr/lib", "usr/lib/i386-linux-gnu"];
#[cfg(all(unix, target_arch = "arm"))]
const LD_PATHS: &[&str] = &["lib", "usr/lib", "usr/lib/arm-linux-gnueabihf"];

/// Absolute forms of `LD_PATHS`, used after `pivot_root` where the process
/// root is the rootfs. `execvp` and the dynamic loader resolve relative PATH
/// / `LD_LIBRARY_PATH` entries against the current directory — with `cwd`
/// set to `/app` that misses `/usr/bin`, so pivot mode must use `/`-prefixed
/// entries (they resolve inside the new root).
#[cfg(unix)]
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
pub struct Metadata {
    name: String,
    #[serde(default)]
    version: Option<String>,
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
    // Read only in the Linux seccomp/landlock setup path; unused elsewhere.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    seccomp: bool,
    #[serde(default)]
    // Read only in the Linux landlock setup path; unused elsewhere.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    landlock: bool,
    #[serde(default)]
    services: Vec<Service>,
    #[serde(default)]
    crypto: Option<CryptoMeta>,
    #[serde(default)]
    payload_format: String,
    #[serde(default)]
    health_check: Option<HealthCheckMeta>,
    #[serde(default)]
    update_url: Option<String>,
}

#[derive(Deserialize)]
pub struct HealthCheckMeta {
    port: u16,
    #[serde(default = "default_health_endpoint")]
    endpoint: String,
    enabled: bool,
}

fn default_health_endpoint() -> String {
    "/health".to_string()
}

#[derive(Deserialize)]
pub struct CryptoMeta {
    nonce_hex: String,
    #[allow(dead_code)]
    tag_offset: usize,
    signing_seed_hex: String,
    encryption_salt_hex: String,
}

#[derive(Deserialize)]
pub struct Service {
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
pub struct Layer {
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

    let (mut exe, mut footer, mut meta_bytes, mut meta) = read_from(&self_exe()?)?;

    // Intercept xbin-reserved runtime flags (`--xbin-update`, `--xbin-version`)
    // before they could reach the host app. Handled modes are terminal: the
    // process exits here without ever exec'ing the app, so the flags are never
    // forwarded.
    handle_runtime_flags(&meta)?;

    // Canonical on-disk path — the file the update engine swaps in place.
    // Kept separate from the running image path because the kernel can pin
    // the pre-swap inode of the running image after a rename.
    let mut bin_path = self_exe()?;

    // SISR self-update: rebuild the binary in place from a signed delta before
    // reading the payload, so this run executes the new version.
    if let Some(updated) = maybe_apply_sisr_update()? {
        if verbose {
            eprintln!("[xbin] SISR update applied: {}", updated.display());
        }
        // Re-open the *canonical real path*, not /proc/self/exe: the kernel can
        // pin the running image's inode, so /proc/self/exe may still resolve to
        // the pre-update file after the rename.
        bin_path = updated;
        (exe, footer, meta_bytes, meta) = read_from(&bin_path)?;
    }

    // 2. Compute cache key and check hit BEFORE reading the payload.
    let layered = footer.format_version >= 2 && !meta.layers.is_empty();
    let hash = if layered {
        crypto::cache_key_v2(&meta.layers)
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
        return exec::exec_app(&meta, &rootfs, &app_config);
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
        crypto::verify_ed25519(&footer, &mut exe, &payload, &meta_bytes)?;
        if verbose {
            eprintln!("[xbin] Ed25519 signature verified");
        }
    }

    // Verify SHA-256 integrity (hash = SHA-256(payload || meta_bytes)).
    let mut buf = payload.clone();
    buf.extend_from_slice(&meta_bytes);
    crypto::verify_sha256(&buf, &footer.payload_sha256)?;

    // Decrypt payload (v4+ with AES-256-GCM).
    // Happens AFTER signature + integrity verification — we only decrypt
    // what's already proven authentic.
    let payload = if footer.crypto_suite() == format::CRYPTO_AES_256_GCM {
        if let Some(ref crypto) = meta.crypto {
            if verbose {
                eprintln!("[xbin] decrypting AES-256-GCM payload");
            }
            let is_sisr = footer.flags & format::FLAG_SISR != 0;
            if is_sisr && !meta.layers.is_empty() {
                let chunk_sizes: Vec<usize> = meta
                    .layers
                    .iter()
                    .map(|l| l.uncompressed_size as usize)
                    .collect();
                crypto::chunked_decrypt_aes_gcm(&payload, crypto, &chunk_sizes)?
            } else {
                crypto::decrypt_aes_gcm(&payload, crypto)?
            }
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
        let _ = gc_extraction_cache(16);
        let is_squashfs = meta.payload_format == format::PAYLOAD_FORMAT_SQUASHFS;
        if is_squashfs {
            let blobs = crypto::slice_layers(&payload, footer.payload_offset, &meta, layered)?;
            extract_squashfs_atomic(&blobs, &cache_root, &rootfs)?;
        } else {
            let blobs = crypto::slice_layers(&payload, footer.payload_offset, &meta, layered)?;
            extract_atomic(&blobs, &cache_root, &rootfs)?;
        }
    }

    // 4. Post-update health gate. A `Pending` record means an update was
    // applied but not yet validated: run the new version supervised and roll
    // back atomically if it fails to start. A `Quarantined` record must never
    // run at all (defense-in-depth on top of the update-time refusal).
    let store = HealthStore::new(&health_store_dir()?);
    let version = footer.sha256_hex();
    let health_status = store.load(&version)?;
    if health_status
        .as_ref()
        .is_some_and(|s| s.state == HealthState::Pending)
    {
        return supervised_launch(&meta, &rootfs, &app_config, &store, &version, &bin_path);
    }
    if health_status
        .as_ref()
        .is_some_and(|s| s.state == HealthState::Quarantined)
    {
        eprintln!(
            "[xbin] version {version} is quarantined after a failed health check; rolling back"
        );
        return rollback_to_previous(&bin_path, verbose);
    }

    if !meta.services.is_empty() {
        exec::supervise_services(&meta, &rootfs, &app_config)
    } else {
        exec::exec_app(&meta, &rootfs, &app_config)
    }
}

/// Opens `path` and reads the footer plus raw and parsed metadata.
fn read_from(path: &Path) -> io::Result<(File, Footer, Vec<u8>, Metadata)> {
    let mut exe = File::open(path)?;
    let footer = Footer::read_from(&mut exe)?;
    let meta_bytes = read_at(&mut exe, footer.meta_offset, footer.meta_size as usize)?;
    let meta: Metadata = serde_json::from_slice(&meta_bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("bad metadata: {e}")))?;
    Ok((exe, footer, meta_bytes, meta))
}

/// Canonical absolute path of the running executable (the .xbin file itself).
/// Linux: readlink(/proc/self/exe); macOS: `_NSGetExecutablePath` via
/// `std::env::current_exe()`. Both are resolved to the on-disk path, which is
/// the file the update engine swaps in place.
fn self_exe() -> io::Result<PathBuf> {
    fs::canonicalize(std::env::current_exe()?)
}

// ---------------------------------------------------------------------------
// SISR self-update
// ---------------------------------------------------------------------------

/// Applies a SISR delta update when `$XBIN_SISR_MANIFEST` points at a signed
/// remote manifest; returns the canonical path of the replaced binary, or
/// `None` when no update was requested.
///
/// Order matters for security: the manifest is authenticated (Ed25519 against
/// the trusted keys, then the Merkle root against its own chunk table) before
/// the engine writes a single byte. Every chunk the engine fetches is
/// additionally hash-verified, and the swap is atomic — any failure leaves the
/// running binary intact.
fn maybe_apply_sisr_update() -> io::Result<Option<PathBuf>> {
    let Some(manifest_path) = std::env::var_os("XBIN_SISR_MANIFEST") else {
        return Ok(None);
    };
    let manifest_path = PathBuf::from(manifest_path);
    let remote_bytes = fs::read(&manifest_path)?;
    let remote = xbin_core::sisr_stage::RemoteManifest::from_bytes(&remote_bytes)?;

    let keys = crypto::load_trusted_keys()?;
    if !remote.verify_any(&keys) {
        return Err(err("update manifest signature verification failed"));
    }
    if !remote.verify_merkle() {
        return Err(err(
            "update manifest Merkle root does not match chunk table",
        ));
    }

    let chunks_root = manifest_path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join("chunks");
    let fetcher = xbin_core::sisr::engine::DirectoryChunkFetcher::new(&chunks_root);

    let current = self_exe()?;
    let store = HealthStore::new(&health_store_dir()?);
    refuse_quarantined_target(&store, &current, &remote.manifest, &fetcher)?;

    let updated = apply_with_rollback_snapshot(&current, &store, |path| {
        xbin_core::sisr::engine::SisrEngine.apply_update(path, &remote.manifest, &fetcher)
    })?;
    Ok(Some(updated))
}

// ---------------------------------------------------------------------------
// Post-update health gate and automatic rollback
// ---------------------------------------------------------------------------

/// Directory holding the per-version health records.
fn health_store_dir() -> io::Result<PathBuf> {
    Ok(cache_dir()?.join("health"))
}

/// The gate's policy: defaults with `XBIN_HEALTH_TIMEOUT_MS` /
/// `XBIN_HEALTH_MAX_ATTEMPTS` overrides (the test harness uses these to make
/// quarantine immediate).
fn health_policy() -> HealthCheckPolicy {
    let mut policy = HealthCheckPolicy::default();
    if let Some(v) = std::env::var("XBIN_HEALTH_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse().ok())
    {
        policy.timeout_ms = v;
    }
    if let Some(v) = std::env::var("XBIN_HEALTH_MAX_ATTEMPTS")
        .ok()
        .and_then(|s| s.parse().ok())
    {
        policy.max_attempts = v;
    }
    policy
}

/// Refuses to apply an update whose *target* version was already quarantined
/// by the health gate — the anti-rollback-loop check. The target hash is
/// expensive (a full dry-run pass), so it is only computed when the store
/// already contains a quarantined version; otherwise this is a no-op.
fn refuse_quarantined_target(
    store: &HealthStore,
    current: &Path,
    manifest: &xbin_core::manifest::DeltaManifest,
    fetcher: &dyn xbin_core::sisr::engine::ChunkFetcher,
) -> io::Result<()> {
    if !store.has_quarantined()? {
        return Ok(());
    }
    let target =
        xbin_core::sisr::engine::SisrEngine.target_payload_sha256(current, manifest, fetcher)?;
    if store.is_quarantined(&hex::encode(target))? {
        return Err(err(
            "update refused: target version failed its health check and is quarantined",
        ));
    }
    Ok(())
}

/// Snapshot → apply → mark-pending, in the right order for a safe rollback.
///
/// The snapshot of the *current* binary is taken before the swap so the gate
/// can restore it later; a failed apply discards the snapshot (the running
/// binary was never touched). A successful apply records the new version as
/// `Pending` so the next launch runs it through the health gate.
fn apply_with_rollback_snapshot(
    current: &Path,
    store: &HealthStore,
    apply: impl FnOnce(&Path) -> io::Result<PathBuf>,
) -> io::Result<PathBuf> {
    let bak = backup_path_for(current);
    create_backup(current, &bak)?;
    let updated = apply(current).inspect_err(|_| {
        let _ = discard_backup(&bak);
    })?;
    mark_pending_after_update(&updated, store)?;
    Ok(updated)
}

/// Records the freshly-swapped binary's version as needing a health check.
fn mark_pending_after_update(updated: &Path, store: &HealthStore) -> io::Result<()> {
    let mut f = File::open(updated)?;
    let footer = Footer::read_from(&mut f)?;
    store.begin(&footer.sha256_hex())?;
    Ok(())
}

/// Outcome of the supervised launch window.
enum ChildStatus {
    StillRunning,
    Exited(i32),
    // Constructed only by the unix waitpid path; matched but never
    // constructed on Windows.
    #[cfg_attr(windows, allow(dead_code))]
    Signaled(i32),
}

/// First launch of a newly-updated version: run the app as a child and watch
/// it for `policy.timeout_ms`.
///
/// - survives the window or exits 0 → healthy: confirm, drop the snapshot,
///   keep supervising until the app exits;
/// - exits non-zero or dies by signal → failure: record it (quarantining
///   after `max_attempts`), restore the pre-update binary from the snapshot,
///   and re-exec it so the user is running a known-good version.
#[cfg(unix)]
fn supervised_launch(
    meta: &Metadata,
    rootfs: &Path,
    app_config: &config::AppConfig,
    store: &HealthStore,
    version_id: &str,
    bin_path: &Path,
) -> io::Result<()> {
    let verbose = std::env::var_os("XBIN_VERBOSE").is_some();
    let policy = health_policy();

    // SAFETY: fork(2) creates a copy of the calling process. The child runs
    // the app (single exec or the service supervisor) and exits with its
    // status; the parent monitors the window and decides confirm vs rollback.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(io::Error::last_os_error());
    }
    if pid == 0 {
        let result = if meta.services.is_empty() {
            exec::exec_app(meta, rootfs, app_config)
        } else {
            exec::supervise_services(meta, rootfs, app_config)
        };
        if let Err(e) = result {
            eprintln!("[xbin] health gate: app failed to start: {e}");
        }
        std::process::exit(127);
    }

    match wait_for_child_status(pid, policy.timeout_ms)? {
        ChildStatus::StillRunning => {
            store.confirm(version_id)?;
            let _ = discard_backup(&backup_path_for(bin_path));
            if verbose {
                eprintln!("[xbin] health gate: version {version_id} healthy");
            }
            exec::install_signal_handler(&[("app".to_string(), pid)]);
            exit(wait_child_exit_code(pid)?);
        }
        ChildStatus::Exited(0) => {
            store.confirm(version_id)?;
            let _ = discard_backup(&backup_path_for(bin_path));
            if verbose {
                eprintln!("[xbin] health gate: version {version_id} healthy (clean exit)");
            }
            exit(0);
        }
        ChildStatus::Exited(code) | ChildStatus::Signaled(code) => {
            eprintln!("[xbin] health gate: version {version_id} failed (exit {code})");
            let quarantined = store.record_failure(version_id, policy.max_attempts)?;
            if quarantined {
                eprintln!(
                    "[xbin] version {version_id} quarantined after {} failed launches",
                    policy.max_attempts
                );
            }
            rollback_to_previous(bin_path, verbose)
        }
    }
}

/// Windows health gate: spawn the app with `CreateProcess` and poll it for
/// `policy.timeout_ms`, with the same confirm-vs-rollback semantics as the
/// unix `fork`/`waitpid` version.
#[cfg(windows)]
fn supervised_launch(
    meta: &Metadata,
    rootfs: &Path,
    app_config: &config::AppConfig,
    store: &HealthStore,
    version_id: &str,
    bin_path: &Path,
) -> io::Result<()> {
    let verbose = std::env::var_os("XBIN_VERBOSE").is_some();
    let policy = health_policy();

    let child = if meta.services.is_empty() {
        exec::spawn_app_windows(meta, rootfs, app_config)?
    } else {
        // Service supervisors share the app spawn path; Windows service
        // supervision inside a health gate is not yet supported.
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "health-gated service supervision is not supported on Windows",
        ));
    };

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(policy.timeout_ms);
    let status = loop {
        match win::try_wait(&child)? {
            Some(0) => break ChildStatus::Exited(0),
            Some(code) => break ChildStatus::Exited(code),
            None if std::time::Instant::now() >= deadline => break ChildStatus::StillRunning,
            None => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    };

    match status {
        ChildStatus::StillRunning | ChildStatus::Exited(0) => {
            store.confirm(version_id)?;
            let _ = discard_backup(&backup_path_for(bin_path));
            if verbose {
                eprintln!("[xbin] health gate: version {version_id} healthy");
            }
            let code = win::wait(&child)?;
            exit(code);
        }
        ChildStatus::Exited(code) => {
            eprintln!("[xbin] health gate: version {version_id} failed (exit {code})");
            let quarantined = store.record_failure(version_id, policy.max_attempts)?;
            if quarantined {
                eprintln!(
                    "[xbin] version {version_id} quarantined after {} failed launches",
                    policy.max_attempts
                );
            }
            rollback_to_previous(bin_path, verbose)
        }
        ChildStatus::Signaled(code) => {
            eprintln!("[xbin] health gate: version {version_id} failed (exit {code})");
            rollback_to_previous(bin_path, verbose)
        }
    }
}

/// Polls `pid` with `WNOHANG` until it exits or `timeout_ms` elapses.
#[cfg(unix)]
fn wait_for_child_status(pid: i32, timeout_ms: u64) -> io::Result<ChildStatus> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        let mut status: i32 = 0;
        // SAFETY: waitpid(2) with WNOHANG polls without blocking; status is
        // written only when the return value equals pid. EINTR is retried.
        let rc = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        if rc == pid {
            return Ok(if libc::WIFSIGNALED(status) {
                ChildStatus::Signaled(128 + libc::WTERMSIG(status))
            } else {
                ChildStatus::Exited(libc::WEXITSTATUS(status))
            });
        }
        if rc < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        if std::time::Instant::now() >= deadline {
            return Ok(ChildStatus::StillRunning);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Blocks until `pid` exits and returns its process exit code.
#[cfg(unix)]
fn wait_child_exit_code(pid: i32) -> io::Result<i32> {
    let mut status: i32 = 0;
    // SAFETY: waitpid(2) blocks until `pid` exits; status is filled by the
    // kernel before the call returns.
    let rc = unsafe { libc::waitpid(pid, &mut status, 0) };
    if rc < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(decode_exit_status(status))
}

#[cfg(unix)]
fn decode_exit_status(status: i32) -> i32 {
    if libc::WIFSIGNALED(status) {
        128 + libc::WTERMSIG(status)
    } else if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else {
        1
    }
}

/// Restores the pre-update binary from its snapshot and execs it.
///
/// The restored file is a self-extracting stub, so exec'ing it re-runs the
/// whole launcher against the old version. Update env vars are cleared first
/// so the manifest is not re-applied into a rollback loop.
fn rollback_to_previous(bin_path: &Path, verbose: bool) -> io::Result<()> {
    let bak = backup_path_for(bin_path);
    if !bak.is_file() {
        return Err(err(&format!(
            "cannot roll back: no snapshot at {}",
            bak.display()
        )));
    }
    restore_backup(bin_path, &bak)?;
    let _ = discard_backup(&bak);
    if verbose {
        eprintln!(
            "[xbin] rolled back to previous version: {}",
            bin_path.display()
        );
    }
    std::env::remove_var("XBIN_SISR_MANIFEST");
    std::env::remove_var("XBIN_UPDATE_URL");
    exec_again(bin_path)
}

/// Re-execs the current stub binary (a `.xbin` file) with the original argv.
#[cfg(unix)]
fn exec_again(bin_path: &Path) -> io::Result<()> {
    let prog = cstr(bin_path.as_os_str().as_bytes())?;
    let mut argv: Vec<CString> = Vec::new();
    argv.push(prog.clone());
    for a in std::env::args_os().skip(1) {
        argv.push(cstr(a.as_bytes())?);
    }
    let argv_ptrs = to_ptr_vec(&argv);
    // SAFETY: execvp(3) replaces the current process; prog is a valid
    // CString, argv_ptrs is null-terminated, env is inherited. Never returns
    // on success.
    unsafe {
        libc_execvp(prog.as_ptr(), argv_ptrs.as_ptr());
    }
    Err(io::Error::last_os_error())
}

/// Re-runs the current stub binary as a detached child and exits (Windows has
/// no exec: the launcher cannot replace its own process image).
#[cfg(windows)]
fn exec_again(bin_path: &Path) -> io::Result<()> {
    let argv: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let env: std::collections::BTreeMap<String, String> = std::env::vars().collect();
    let child = win::spawn(bin_path, &argv, &env, None, true)?;
    let _ = child.pid;
    exit(0);
}

// ---------------------------------------------------------------------------
// `--xbin-update` / `--xbin-version` runtime flags
// ---------------------------------------------------------------------------

/// Intercepts the xbin-reserved runtime flags and handles them terminally.
///
/// - `--xbin-version` prints version info on stdout and exits 0.
/// - `--xbin-update [URL]` fetches the signed remote manifest and the changed
///   chunks from the update channel, applies the delta atomically, prints
///   reuse/fetch statistics on stderr, and exits 0.
///
/// Because both paths call `process::exit`, these flags never reach the host
/// app's `argv`. When neither flag is present this is a no-op.
fn handle_runtime_flags(meta: &Metadata) -> io::Result<()> {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();

    if args.iter().any(|a| a == "--xbin-version") {
        println!("xbin {} (stub)", env!("CARGO_PKG_VERSION"));
        if let Some(v) = &meta.version {
            println!("app version: {v}");
        }
        exit(0);
    }

    if let Some(idx) = args.iter().position(|a| a == "--xbin-update") {
        let base = resolve_update_url(&args, idx, meta)?;
        remote_update(&base)?;
        exit(0);
    }

    Ok(())
}

/// Resolves the update channel base URL:
/// `--xbin-update <URL>` argument > `$XBIN_UPDATE_URL` > embedded `meta.update_url`.
fn resolve_update_url(
    args: &[std::ffi::OsString],
    idx: usize,
    meta: &Metadata,
) -> io::Result<String> {
    update_url::resolve_update_url(args, idx, meta)
}

/// Fetches `<base>/manifest` (XBMR), authenticates it against the trusted
/// keys + Merkle root, then streams the changed chunks from `<base>/chunks/<hex>`
/// through the engine. Progress and reuse/fetch stats go to stderr; the
/// process exits after the atomic swap.
fn remote_update(base: &str) -> io::Result<()> {
    eprintln!("[xbin] update: fetching manifest from {base}/manifest");
    let manifest_bytes = http_get_bytes(&format!("{base}/manifest"))?;
    let remote = xbin_core::sisr_stage::RemoteManifest::from_bytes(&manifest_bytes)?;

    let keys = crypto::load_trusted_keys()?;
    if !remote.verify_any(&keys) {
        return Err(err("update manifest signature verification failed"));
    }
    if !remote.verify_merkle() {
        return Err(err(
            "update manifest Merkle root does not match chunk table",
        ));
    }

    let total = remote.manifest.chunks.len();
    eprintln!("[xbin] update: manifest verified ({total} chunks)");

    let current = self_exe()?;
    let store = HealthStore::new(&health_store_dir()?);
    let fetcher = HttpChunkFetcher::new(&format!("{base}/chunks"), total);
    refuse_quarantined_target(&store, &current, &remote.manifest, &fetcher)?;

    let updated = apply_with_rollback_snapshot(&current, &store, |path| {
        let (updated, stats) = xbin_core::sisr::engine::SisrEngine.apply_update_with_stats(
            path,
            &remote.manifest,
            &fetcher,
        )?;
        eprintln!(
            "[xbin] update applied: {} chunks reused ({}), {} chunks fetched ({}), total {total}",
            stats.reused_chunks,
            human_bytes(stats.reused_bytes),
            stats.fetched_chunks,
            human_bytes(stats.fetched_bytes),
        );
        Ok(updated)
    })?;
    eprintln!("[xbin] updated binary: {}", updated.display());
    Ok(())
}

/// [`ChunkFetcher`] pulling chunks from `<base>/<64-hex-sha256>` over HTTPS.
///
/// Content-addressability is the security anchor: every chunk the engine
/// writes must SHA-256 to its manifest entry, so the transport cannot smuggle
/// a wrong chunk in. The fetcher only counts + reports progress.
struct HttpChunkFetcher {
    base: String,
    total: usize,
    done: std::cell::Cell<usize>,
    bytes: std::cell::Cell<u64>,
}

impl HttpChunkFetcher {
    fn new(base: &str, total: usize) -> Self {
        Self {
            base: base.to_string(),
            total,
            done: std::cell::Cell::new(0),
            bytes: std::cell::Cell::new(0),
        }
    }
}

impl xbin_core::sisr::engine::ChunkFetcher for HttpChunkFetcher {
    fn fetch(&self, hash: &[u8; 32], length: usize) -> io::Result<Vec<u8>> {
        let url = format!("{}/{}", self.base, hex::encode(hash));
        let bytes = http_get_bytes(&url)?;
        if bytes.len() != length {
            return Err(err("fetched chunk length mismatch"));
        }
        let done = self.done.get() + 1;
        self.done.set(done);
        self.bytes
            .set(self.bytes.get().saturating_add(bytes.len() as u64));
        eprintln!(
            "[xbin]   fetched chunk {done}/{} ({} bytes)",
            self.total,
            bytes.len()
        );
        Ok(bytes)
    }

    fn bytes_fetched(&self) -> u64 {
        self.bytes.get()
    }
}

/// Minimal HTTPS GET returning the raw response body.
///
/// Only caller-verified content is consumed (signed manifest, hash-checked
/// chunks), so the transport is a convenience — never a trust anchor.
fn http_get_bytes(url: &str) -> io::Result<Vec<u8>> {
    let resp = ureq::get(url)
        .call()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("GET {url}: {e}")))?;
    resp.into_body()
        .read_to_vec()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("GET {url}: {e}")))
}

fn human_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    if bytes >= MIB {
        let whole = bytes / MIB;
        let frac = (bytes % MIB) * 10 / MIB;
        format!("{whole}.{frac} MiB")
    } else if bytes >= KIB {
        let whole = bytes / KIB;
        let frac = (bytes % KIB) * 10 / KIB;
        format!("{whole}.{frac} KiB")
    } else {
        format!("{bytes} B")
    }
}

/// Platform cache root for extracted rootfs trees.
/// Linux: `$XDG_CACHE_HOME/xbin` or `~/.cache/xbin`.
/// macOS: `$XDG_CACHE_HOME/xbin` if set, else `~/Library/Caches/xbin`
/// (`dirs::cache_dir()`).
/// Windows: `%LOCALAPPDATA%\xbin`.
fn cache_dir() -> io::Result<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(xdg).join("xbin"));
    }
    #[cfg(target_os = "windows")]
    {
        let local = std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA not set"))?;
        Ok(PathBuf::from(local).join("xbin"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let dir = dirs::cache_dir().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "no cache directory available")
        })?;
        Ok(dir.join("xbin"))
    }
}

/// Garbage-collect the extracted rootfs cache, keeping at most `max_entries`
/// directories (LRU by `.ready` mtime). Called before a cold extraction so
/// the cache does not grow without bound.
///
/// The extraction cache lives under `cache_dir()/{hash}/rootfs/`. Each entry
/// has a `.ready` marker whose mtime is updated on every warm hit, giving a
/// cheap LRU signal without extra metadata files.
fn gc_extraction_cache(max_entries: usize) -> io::Result<()> {
    let base = cache_dir()?;
    let mut entries: Vec<_> = match fs::read_dir(&base) {
        Ok(iter) => iter.filter_map(Result::ok).collect(),
        Err(_) => return Ok(()),
    };
    entries.retain(|e| e.path().is_dir() && e.path().join("rootfs").is_dir());
    if entries.len() <= max_entries {
        return Ok(());
    }
    entries.sort_by_key(|e| {
        e.path()
            .join(".ready")
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH)
    });
    while entries.len() > max_entries {
        if let Some(oldest) = entries.first() {
            let _ = fs::remove_dir_all(oldest.path());
            entries.remove(0);
        }
    }
    Ok(())
}

fn extract_atomic(blobs: &[&[u8]], cache_root: &Path, rootfs: &Path) -> io::Result<()> {
    extraction::extract_atomic(blobs, cache_root, rootfs)
}

fn extract_squashfs_atomic(blobs: &[&[u8]], cache_root: &Path, rootfs: &Path) -> io::Result<()> {
    extraction::extract_squashfs_atomic(blobs, cache_root, rootfs)
}

// ---------------------------------------------------------------------------
// Seccomp BPF denylist
// ---------------------------------------------------------------------------
#[cfg(target_os = "linux")]
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

#[cfg(unix)]
fn cstr(bytes: &[u8]) -> io::Result<CString> {
    CString::new(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "path contains null byte"))
}

#[cfg(unix)]
fn to_ptr_vec(v: &[CString]) -> Vec<*const core::ffi::c_char> {
    let mut ptrs: Vec<*const core::ffi::c_char> = v.iter().map(|c| c.as_ptr()).collect();
    ptrs.push(std::ptr::null());
    ptrs
}

pub fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// Acquire an exclusive advisory lock (flock(2)) on `f`.
#[cfg(unix)]
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

/// Windows has no flock(2). The extraction lock protects concurrent cold
/// starts of the same binary; the atomic tmp→cache rename means both writers
/// produce identical content, so a lost lock only wastes duplicate work.
#[cfg(windows)]
fn flock_exclusive(_f: &File) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_formats_all_scales() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(1536), "1.5 KiB");
        assert_eq!(human_bytes(1024 * 1024), "1.0 MiB");
        assert_eq!(human_bytes((1024 * 1024) + (512 * 1024)), "1.5 MiB");
    }
}
