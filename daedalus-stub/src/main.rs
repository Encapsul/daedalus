#![allow(missing_docs)]
//! daedalus launcher stub.
//!
//! Embedded at the head of every .daedalus file — this is the ELF the kernel runs.
//! Flow: open /proc/self/exe → read footer → verify integrity (sig → SHA-256) →
//! extract rootfs to ~/.cache/daedalus/{sha256}/ (atomic) → exec the app.
//!
//! Isolation: level 0 = `LD_LIBRARY_PATH` (no sandbox), level 2 = user +
//! mount namespaces with `pivot_root` into the extracted rootfs and
//! optional seccomp BPF denylist. See `enter_namespace_if_needed()`,
//! `pivot_root_into()`, and `install_seccomp_denylist()`.

#![warn(missing_docs)]

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
#[cfg(target_os = "linux")]
mod update_url;
#[cfg(target_os = "windows")]
mod win;

use daedalus_core::detect;
use daedalus_core::encrypt::decrypt_payload;
use daedalus_core::format::{self as format, read_at, Footer};
use daedalus_core::layer::SerializableLayer;
use daedalus_core::sisr::health::{HealthCheckPolicy, HealthState, HealthStore};
use daedalus_core::sisr::resilience::{
    backup_path_for, create_backup, discard_backup, restore_backup,
};
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::ffi::CString;
use std::fs::{self, File};
use std::io::{self, Read, Seek};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::exit;

/// Standard library search paths for `LD_LIBRARY_PATH`.
/// Mirrors the search path construction of the packaged rootfs layout.
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
#[cfg(all(unix, target_arch = "riscv64"))]
const LD_PATHS: &[&str] = &[
    "lib",
    "lib64",
    "usr/lib",
    "usr/lib64",
    "usr/lib/riscv64-linux-gnu",
];

/// Absolute forms of `LD_PATHS`, used after `pivot_root` where the process
/// root is the rootfs. `execvp` and the dynamic loader resolve relative PATH
/// / `LD_LIBRARY_PATH` entries against the current directory — with `cwd`
/// set to `/app` that misses `/usr/bin`, so pivot mode must use `/`-prefixed
/// entries (they resolve inside the new root).
#[cfg(all(unix, target_arch = "x86_64"))]
const LD_PATHS_ABS: &[&str] = &[
    "/lib",
    "/lib64",
    "/usr/lib",
    "/usr/lib64",
    "/usr/lib/x86_64-linux-gnu",
];
#[cfg(all(unix, target_arch = "aarch64"))]
const LD_PATHS_ABS: &[&str] = &[
    "/lib",
    "/lib64",
    "/usr/lib",
    "/usr/lib64",
    "/usr/lib/aarch64-linux-gnu",
];
#[cfg(all(unix, not(any(target_arch = "x86_64", target_arch = "aarch64"))))]
const LD_PATHS_ABS: &[&str] = &["/lib", "/lib64", "/usr/lib", "/usr/lib64"];

/// Binary search paths for PATH, mirroring `LD_PATHS` for executables.
/// Bundled binaries (e.g. ffmpeg, gitleaks) land here via the rootfs.
const BIN_PATHS: &[&str] = &["usr/bin", "bin", "usr/local/bin"];

/// Absolute forms of `BIN_PATHS`; see `LD_PATHS_ABS`.
const BIN_PATHS_ABS: &[&str] = &["/usr/bin", "/bin", "/usr/local/bin"];

#[derive(Deserialize)]
/// `Metadata` - metadata block embedded in a `.de` binary.
///
/// Description:
/// Contains app metadata extracted from the footer at runtime.
/// Used by the launcher to configure execution environment.
///
/// Return: nothing
pub struct Metadata {
    /// `name` - application name.
    ///
    /// Description:
    /// Human-readable name of the packaged application.
    ///
    /// Return: nothing
    name: String,
    #[serde(default)]
    /// `version` - application version.
    ///
    /// Description:
    /// Optional semantic version from the app manifest.
    ///
    /// Return: nothing
    version: Option<String>,
    #[serde(default)]
    /// `runtime` - runtime identifier.
    ///
    /// Description:
    /// Runtime name, e.g. `python`, `node`, `go`.
    ///
    /// Return: nothing
    runtime: String,
    #[serde(default)]
    /// `entrypoint` - executable and arguments.
    ///
    /// Description:
    /// argv for the target process, relative to rootfs or absolute.
    ///
    /// Return: nothing
    entrypoint: Vec<String>,
    #[serde(default)]
    /// `env` - environment variables.
    ///
    /// Description:
    /// Key-value pairs injected into the target process environment.
    ///
    /// Return: nothing
    env: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    /// `cwd` - working directory.
    ///
    /// Description:
    /// Optional working directory for the target process.
    /// Defaults to `/app` if unset.
    ///
    /// Return: nothing
    cwd: Option<String>,
    #[serde(default)]
    /// `isolation` - isolation level.
    ///
    /// Description:
    /// 0 = none, 1 = namespace, 2 = full sandbox.
    ///
    /// Return: nothing
    pub isolation: u8,
    #[serde(default)]
    // Read only in the Linux landlock setup path; unused elsewhere.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    /// `cpu_limit` - CPU quota in millicores.
    ///
    /// Description:
    /// Optional CPU limit applied via cgroups on Linux.
    ///
    /// Return: nothing
    pub cpu_limit: Option<u32>,
    #[serde(default)]
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    /// `memory_limit_mb` - memory limit in megabytes.
    ///
    /// Description:
    /// Optional memory limit applied via cgroups on Linux.
    ///
    /// Return: nothing
    pub memory_limit_mb: Option<u32>,
    #[serde(default)]
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    /// `pid_limit` - maximum number of child processes.
    ///
    /// Description:
    /// Optional PID limit applied via cgroups on Linux.
    ///
    /// Return: nothing
    pub pid_limit: Option<u32>,
    #[serde(default)]
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    /// `seccomp` - enable seccomp filter.
    ///
    /// Description:
    /// When true, install a seccomp-BPF denylist before exec.
    ///
    /// Return: nothing
    pub seccomp: bool,
    #[serde(default)]
    // Read only in the Linux landlock setup path; unused elsewhere.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    /// `landlock` - enable Landlock filesystem sandbox.
    ///
    /// Description:
    /// When true, restrict file system access to the rootfs only.
    /// Linux-only, kernel >= 5.13.
    ///
    /// Return: nothing
    landlock: bool,
    #[serde(default)]
    // Read only in the Linux GUI bind-mount path (pivot_root isolation).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    /// `gui` - enable GUI support.
    ///
    /// Description:
    /// When true, bind-mount /dev, /tmp, and other GUI paths.
    ///
    /// Return: nothing
    gui: bool,
    #[serde(default)]
    /// `services` - multi-service definitions.
    ///
    /// Description:
    /// Additional services to supervise alongside the main entrypoint.
    ///
    /// Return: nothing
    services: Vec<Service>,
    #[serde(default)]
    /// `payload_format` - payload compression format.
    ///
    /// Description:
    /// `zstd` for tar+zstd, `squashfs` for SquashFS image.
    ///
    /// Return: nothing
    payload_format: String,
    #[serde(default)]
    /// `health_check` - health check configuration.
    ///
    /// Description:
    /// Optional built-in health check server settings.
    ///
    /// Return: nothing
    health_check: Option<HealthCheckMeta>,
    #[serde(default)]
    #[allow(dead_code)]
    /// `update_url` - self-update check URL.
    ///
    /// Description:
    /// URL to query for newer versions. When set, the launcher
    /// checks for updates on startup.
    ///
    /// Return: nothing
    update_url: Option<String>,
    #[serde(default)]
    /// `hooks` - lifecycle hooks.
    ///
    /// Description:
    /// Optional pre/post hooks for the main service.
    ///
    /// Return: nothing
    hooks: Option<Hooks>,
    /// Layers composing this artifact. Empty for legacy binaries.
    #[serde(default)]
    layers: Vec<SerializableLayer>,
    /// Name of the layer containing the main entrypoint.
    #[serde(default)]
    entrypoint_layer: Option<String>,
    #[serde(default)]
    encryption: Option<daedalus_core::metadata::EncryptionMeta>,
}

impl Metadata {
    /// Returns the effective entrypoint argv template, preferring the layer
    /// system when `entrypoint_layer` is set, falling back to the flat
    /// `entrypoint` field for legacy binaries.
    pub fn effective_entrypoint(&self) -> &[String] {
        if let Some(layer_name) = &self.entrypoint_layer {
            if let Some(daedalus_core::layer::SerializableLayer::Runtime(rt)) =
                self.layers.iter().find(|l| l.name() == layer_name)
            {
                return &rt.entrypoint;
            }
        }
        &self.entrypoint
    }

    /// Returns the effective runtime name, preferring the layer system when
    /// `entrypoint_layer` is set, falling back to the flat `runtime` field.
    pub fn effective_runtime(&self) -> &str {
        if let Some(layer_name) = &self.entrypoint_layer {
            if let Some(daedalus_core::layer::SerializableLayer::Runtime(rt)) =
                self.layers.iter().find(|l| l.name() == layer_name)
            {
                return &rt.name;
            }
        }
        &self.runtime
    }

    /// Returns the capabilities requested by the entrypoint layer. If
    /// `entrypoint_layer` points to a `RuntimeLayer`, its `capabilities` vec
    /// is returned; otherwise falls back to an empty slice (legacy binaries
    /// with no layer system).
    pub fn effective_capabilities(&self) -> &[daedalus_core::layer::Capability] {
        if let Some(layer_name) = &self.entrypoint_layer {
            if let Some(daedalus_core::layer::SerializableLayer::Runtime(rt)) =
                self.layers.iter().find(|l| l.name() == layer_name)
            {
                return &rt.capabilities;
            }
        }
        &[]
    }
}

/// Layer manifest written to cache after extraction.
///
/// Records which layers compose the extracted rootfs, enabling layer-aware
/// cache validation (warm start), hot-swap, and lazy-loading (Phases 1-3).
/// Stored as `cache_root/.daedalus-layers.json`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LayerManifest {
    /// `version` - manifest format version.
    ///
    /// Description:
    /// Current manifest version is 1.
    ///
    /// Return: nothing
    pub version: u8,
    /// `layers` - ordered layer entries.
    ///
    /// Description:
    /// Layers are applied in order during extraction.
    ///
    /// Return: nothing
    pub layers: Vec<LayerManifestEntry>,
}

/// A single entry in the layer manifest.
///
/// Mirrors the relevant subset of `SerializableLayer` for cache tracking.
/// The `rootfs_path` is where the layer's content lives inside the extracted
/// rootfs (e.g. `/app` for a `RuntimeLayer`, custom path for a `Custom` layer).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LayerManifestEntry {
    /// `name` - layer name.
    ///
    /// Description:
    /// Human-readable name for the layer.
    ///
    /// Return: nothing
    pub name: String,
    /// `kind` - layer kind.
    ///
    /// Description:
    /// Layer type identifier, e.g. `Runtime`, `App`, `Custom`.
    ///
    /// Return: nothing
    pub kind: String,
    /// `rootfs_path` - mount point inside rootfs.
    ///
    /// Description:
    /// Absolute path where this layer's content is mounted.
    /// None means the layer is merged at the root.
    ///
    /// Return: nothing
    pub rootfs_path: Option<String>,
}

impl LayerManifest {
    /// `from_metadata` - from metadata.
    /// `@meta`: metadata
    ///
    /// Description:
    ///
    /// Return: the `Self`
    fn from_metadata(meta: &Metadata) -> Self {
        let layers = meta
            .layers
            .iter()
            .map(|layer| {
                let rootfs_path = layer_to_rootfs_path(layer);
                LayerManifestEntry {
                    name: layer.name().to_string(),
                    kind: layer_kind_str(layer.kind()),
                    rootfs_path,
                }
            })
            .collect();
        LayerManifest { version: 1, layers }
    }
}

/// `layer_kind_str` - layer kind str.
/// `@kind`: kind
/// `@daedalus_core`: daedalus core
/// `@layer`: layer
///
/// Description:
///
/// Return: the `resulting` string
fn layer_kind_str(kind: daedalus_core::layer::LayerKind) -> String {
    match kind {
        daedalus_core::layer::LayerKind::Runtime => "runtime",
        daedalus_core::layer::LayerKind::Config => "config",
        daedalus_core::layer::LayerKind::Custom => "custom",
    }
    .to_string()
}

/// Resolve the primary rootfs path for a layer type.
fn layer_to_rootfs_path(layer: &daedalus_core::layer::SerializableLayer) -> Option<String> {
    match layer {
        daedalus_core::layer::SerializableLayer::Runtime(_)
        | daedalus_core::layer::SerializableLayer::Config(_) => Some("/app".to_string()),
        daedalus_core::layer::SerializableLayer::Custom { extra, .. } => {
            extra.get("path").and_then(|v| v.as_str()).map(String::from)
        }
    }
}

/// Write the layer manifest to the cache after extraction.
///
/// # Errors
///
/// Returns `io::Error` if the manifest JSON cannot be serialized or the
/// cache directory is not writable.
pub fn write_layer_manifest(cache_root: &Path, meta: &Metadata) -> std::io::Result<()> {
    let manifest = LayerManifest::from_metadata(meta);
    let path = cache_root.join(".daedalus-layers.json");
    let json = serde_json::to_string(&manifest)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

/// Load the layer manifest from cache (warm start), returning None if absent.
pub fn load_layer_manifest(cache_root: &Path) -> Option<LayerManifest> {
    let path = cache_root.join(".daedalus-layers.json");
    let data = std::fs::read(&path).ok()?;
    serde_json::from_slice(&data).ok()
}

#[derive(Deserialize)]
/// `HealthCheckMeta` - health check metadata.
///
/// Description:
/// Configuration for the built-in health check HTTP server.
///
/// Return: nothing
pub struct HealthCheckMeta {
    /// `port` - TCP port for the health server.
    ///
    /// Description:
    /// Port on which the health check HTTP server listens.
    ///
    /// Return: nothing
    port: u16,
    #[serde(default = "default_health_endpoint")]
    /// `endpoint` - health check endpoint path.
    ///
    /// Description:
    /// HTTP path for the liveness/readiness probe.
    ///
    /// Return: nothing
    endpoint: String,
    /// `enabled` - whether health checks are enabled.
    ///
    /// Description:
    /// When false, the health server is not started.
    ///
    /// Return: nothing
    enabled: bool,
}

/// `default_health_endpoint` - get default health endpoint.
///
/// Description:
///
/// Return: the `resulting` string
fn default_health_endpoint() -> String {
    "/health".to_string()
}

#[derive(Deserialize)]
/// `Service` - a service definition for multi-service mode.
///
/// Description:
/// Represents a service to be supervised. Each service has a name,
/// command, optional environment variables, and optional readiness probe.
///
/// Return: nothing
pub struct Service {
    /// `name` - service name.
    ///
    /// Description:
    /// Human-readable name used in logs and process titles.
    ///
    /// Return: nothing
    name: String,
    /// `cmd` - command and arguments.
    ///
    /// Description:
    /// Executable and its arguments, relative to the rootfs.
    ///
    /// Return: nothing
    cmd: Vec<String>,
    #[serde(default)]
    /// `env` - environment variables.
    ///
    /// Description:
    /// Additional environment variables for this service.
    ///
    /// Return: nothing
    env: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    /// `ready_port` - TCP port for readiness probe.
    ///
    /// Description:
    /// If nonzero, daedalus waits for TCP connectivity on this port
    /// before marking the service ready.
    ///
    /// Return: nothing
    ready_port: u16,
    #[serde(default)]
    /// `ready_timeout` - readiness timeout in seconds.
    ///
    /// Description:
    /// Maximum time to wait for the readiness probe before failing.
    ///
    /// Return: nothing
    ready_timeout: u64,
}

#[derive(serde::Deserialize, Default)]
/// `Hooks` - lifecycle hooks for sidecars.
///
/// Description:
/// Optional commands to run before (`pre`) or after (`post`) a sidecar
/// starts. Used for migrations, health checks, or cleanup.
///
/// Return: nothing
pub struct Hooks {
    /// `pre` - commands to run before the sidecar starts.
    ///
    /// Description:
    /// Executed in order before the sidecar process is spawned.
    /// Failure aborts the sidecar startup.
    ///
    /// Return: nothing
    #[serde(default)]
    pub pre: Vec<String>,
    /// `post` - commands to run after the sidecar stops.
    ///
    /// Description:
    /// Executed in order after the sidecar process exits.
    /// Failures are logged but do not affect the exit code.
    ///
    /// Return: nothing
    #[serde(default)]
    pub post: Vec<String>,
}

/// `main` - main.
///
/// Description:
///
/// Return: nothing
fn main() {
    if let Err(e) = run() {
        eprintln!("[daedalus] error: {e}");
        exit(1);
    }
}

/// `run` - run.
/// `@io`: io
///
/// Description:
///
/// Return: Result containing `io::Result<()>`
fn run() -> io::Result<()> {
    let verbose = std::env::var_os("DAEDALUS_VERBOSE").is_some();

    let args: Vec<String> = std::env::args().collect();
    let decrypt_key = args
        .windows(2)
        .find(|w| w[0] == "--decrypt-key")
        .map(|w| w[1].clone());

    // Load configuration (multi-layered: CLI args → local config → env vars → global config)
    let app_config = config::AppConfig::load();

    let (mut exe, mut footer, mut meta_bytes, mut meta) = read_from(&self_exe()?)?;

    // Reject crafted metadata with an unknown runtime before doing any
    // extraction/update work (roadmap #40 — unknown runtime used to silently
    // map to bash). Validation goes through `effective_runtime()` so a binary
    // whose entrypoint lives in a layer is checked against THAT layer's
    // runtime, not the legacy flat field.
    let runtime = meta.effective_runtime().to_string();
    if detect::Runtime::from_name(&runtime).is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported runtime '{runtime}' in metadata — supported: python, deno, node, electron, java, ruby, dotnet, rust, go, php, perl, hugo, wasm, binary",
            ),
        ));
    }

    // Intercept daedalus-reserved runtime flags (`--daedalus-update`, `--daedalus-version`)
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
            eprintln!("[daedalus] SISR update applied: {}", updated.display());
        }
        // Re-open the *canonical real path*, not /proc/self/exe: the kernel can
        // pin the running image's inode, so /proc/self/exe may still resolve to
        // the pre-update file after the rename.
        bin_path = updated;
        (exe, footer, meta_bytes, meta) = read_from(&bin_path)?;
    }

    // 2. Compute cache key and check hit BEFORE reading the payload.
    let hash = footer.sha256_hex();

    let base = cache_dir()?;
    fs::create_dir_all(&base)?;
    let cache_root = base.join(&hash);
    let rootfs = cache_root.join("rootfs");
    let ready_marker = cache_root.join(".ready");

    if ready_marker.exists() && extraction::cache_root_trustworthy(&cache_root) {
        // Verify the on-disk source still matches what was extracted:
        // a byte-flip in the payload would keep the footer's SHA-256
        // cache key unchanged, so we additionally bind the cache to the
        // source file's size + mtime to detect tampering.
        let source_meta = fs::metadata(&bin_path)
            .unwrap_or_else(|_| fs::metadata(self_exe().unwrap_or_default()).unwrap());

        let src_valid = extraction::source_manifest_matches(
            &cache_root,
            source_meta.len(),
            source_meta
                .modified()
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
        );

        if src_valid {
            if verbose {
                eprintln!("[daedalus] warm start: cache hit {}", hash);
            }
            return exec::exec_app(&meta, &rootfs, &app_config);
        }

        // Source changed since last extraction — fall through to cold path
        // which will re-verify SHA-256 + signature and rebuild the cache.
        if verbose {
            eprintln!("[daedalus] source changed since cache — re-verifying");
        }
    }

    // 3. Cold path: read payload + verify + extract.
    if verbose {
        eprintln!("[daedalus] cold start: extracting {}", meta.name);
    }

    let payload = read_at(
        &mut exe,
        footer.payload_offset,
        footer.payload_csize as usize,
    )?;

    let (payload, meta_bytes) = if let Some(ref enc) = meta.encryption {
        let key = decrypt_key.as_ref().ok_or_else(|| {
            err("payload is encrypted — pass --decrypt-key <32-byte-hex-keyfile> to decrypt")
        })?;
        let key_bytes =
            hex::decode(key).map_err(|e| err(format!("invalid decrypt key hex: {e}")))?;
        if key_bytes.len() != 32 {
            return Err(err(format!(
                "decrypt key must be 32 bytes, got {}",
                key_bytes.len()
            )));
        }
        let mut key_array = [0u8; 32];
        key_array.copy_from_slice(&key_bytes);
        let salt =
            hex::decode(&enc.salt).map_err(|e| err(format!("bad encryption salt hex: {e}")))?;
        let nonce =
            hex::decode(&enc.nonce).map_err(|e| err(format!("bad encryption nonce hex: {e}")))?;
        if salt.len() != 32 || nonce.len() != 12 {
            return Err(err("encryption metadata has invalid salt/nonce length"));
        }
        let mut salt_array = [0u8; 32];
        let mut nonce_array = [0u8; 12];
        salt_array.copy_from_slice(&salt);
        nonce_array.copy_from_slice(&nonce);
        let plaintext = decrypt_payload(&payload, &key_array, &nonce_array, &salt_array)
            .map_err(|e| err(format!("decryption failed: {e}")))?;
        if verbose {
            eprintln!("[daedalus] payload decrypted ({} bytes)", plaintext.len());
        }
        // Recompute metadata hash with decrypted payload
        let new_meta = meta_bytes.clone();
        (plaintext, new_meta)
    } else {
        (payload, meta_bytes)
    };

    // Verify Ed25519 signature. Enforce a consistent signature state first:
    // a sig block must exist iff FLAG_SIGNED is set — a flag without a block
    // (or a block without the flag) is a tampered file. The signature covers
    // the footer itself, so rewriting format_version/flags to skip it breaks
    // the signature; a v2 file that still carries the leftover sig block from
    // a downgraded v3+ file is rejected outright.
    let has_sig_block = footer.format_version >= 3 && footer.sig_offset != 0;
    let signed_flag = footer.flags & format::FLAG_SIGNED != 0;
    if has_sig_block != signed_flag {
        return Err(err("inconsistent signature state (flag/offset mismatch)"));
    }
    if has_sig_block {
        crypto::verify_ed25519(&footer, &mut exe, &payload, &meta_bytes)?;
        if verbose {
            eprintln!("[daedalus] Ed25519 signature verified");
        }
    } else if footer.format_version < 3 && !footer.has_sisr() {
        reject_downgraded_sig_block(&mut exe, &footer)?;
    }

    // Verify SHA-256 integrity (hash = SHA-256(payload || meta_bytes)).
    // Stream the two slices into the hasher instead of cloning payload,
    // avoiding a 2× memory spike at cold start.
    crypto::verify_sha256_parts(&payload, &meta_bytes, &footer.payload_sha256)?;

    // At-rest authenticity for SISR binaries (roadmap #45): the embedded
    // delta manifest must carry a valid publisher signature and its chunk
    // table must bind every byte of the payload. Updates keep this property
    // because the engine carries over the remote manifest's verified
    // signature. `DAEDALUS_SISR_ALLOW_UNSIGNED` is the explicit escape hatch for
    // legacy unsigned builds — fail closed otherwise.
    if footer.has_sisr() && std::env::var_os("DAEDALUS_SISR_ALLOW_UNSIGNED").is_none() {
        let (ext, sisr_manifest) = daedalus_core::sisr_header::read_sisr(&mut exe)?
            .ok_or_else(|| err("SISR flag set but section unreadable"))?;
        let keys = crypto::load_trusted_keys()?;
        daedalus_core::sisr_stage::verify_embedded_sisr(&ext, &sisr_manifest, &payload, &keys)?;
        if verbose {
            eprintln!("[daedalus] SISR section verified (at-rest)");
        }
    }

    // Extract atomically.
    let lock = File::create(base.join(format!("{hash}.lock")))?;
    flock_exclusive(&lock)?;

    if !ready_marker.exists() {
        let gc_limit = std::env::var("DAEDALUS_CACHE_MAX_ENTRIES")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(16);
        let _ = gc_extraction_cache(gc_limit);
        let is_squashfs = meta.payload_format == format::PAYLOAD_FORMAT_SQUASHFS;
        if is_squashfs {
            extract_squashfs_atomic(&[payload.as_slice()], &cache_root)?;
        } else {
            extract_atomic(&[payload.as_slice()], &cache_root)?;
        }
        write_layer_manifest(&cache_root, &meta)?;
    }

    // Record source identity after every successful verification/extraction
    // so the warm path can detect payload tampering (byte-flips that don't
    // change the footer's SHA-256 cache key).
    let src_meta = fs::metadata(&bin_path)
        .unwrap_or_else(|_| fs::metadata(self_exe().unwrap_or_default()).unwrap());
    let _ = extraction::write_source_manifest(
        &cache_root,
        src_meta.len(),
        src_meta
            .modified()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
    );

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
            "[daedalus] version {version} is quarantined after a failed health check; rolling back"
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

/// A v3+ signed file downgraded to v2 keeps its 68-byte signature block in
/// the otherwise-empty gap between the metadata and the 84-byte footer. Two
/// shapes exist: the 8-byte `sig_offset` prefix may have been stripped with
/// the footer rewrite (`meta_end + 68 + 84`) or left in place (`meta_end +
/// 68 + 92`). Either gap with a plausible sig-size field at `meta_end` is a
/// downgrade attempt, not a legitimate build (a real build ends the metadata
/// flush at the footer).
fn reject_downgraded_sig_block<R: Read + Seek>(exe: &mut R, footer: &Footer) -> io::Result<()> {
    let file_len = exe.seek(io::SeekFrom::End(0))?;
    let meta_end = footer
        .meta_offset
        .checked_add(footer.meta_size)
        .ok_or_else(|| err("metadata region overflows u64"))?;
    let gap_with_prefix = format::SIG_BLOCK_SIZE as u64 + format::V3_FOOTER_SIZE;
    let gap_stripped = format::SIG_BLOCK_SIZE as u64 + format::V2_FOOTER_SIZE;
    if file_len != meta_end + gap_with_prefix && file_len != meta_end + gap_stripped {
        return Ok(());
    }
    let mut size_buf = [0u8; 4];
    exe.seek(io::SeekFrom::Start(meta_end))?;
    exe.read_exact(&mut size_buf)?;
    if u32::from_le_bytes(size_buf) as usize == format::SIG_LEN {
        return Err(err(
            "rejected: leftover signature block (downgraded signed binary)",
        ));
    }
    Ok(())
}

/// Canonical absolute path of the running executable (the .daedalus file itself).
/// Linux: readlink(/proc/self/exe); macOS: `_NSGetExecutablePath` via
/// `std::env::current_exe()`. Both are resolved to the on-disk path, which is
/// the file the update engine swaps in place.
fn self_exe() -> io::Result<PathBuf> {
    fs::canonicalize(std::env::current_exe()?)
}

// ---------------------------------------------------------------------------
// SISR self-update
// ---------------------------------------------------------------------------

/// Applies a SISR delta update when `$DAEDALUS_SISR_MANIFEST` points at a signed
/// remote manifest; returns the canonical path of the replaced binary, or
/// `None` when no update was requested.
///
/// Order matters for security: the manifest is authenticated (Ed25519 against
/// the trusted keys, then the Merkle root against its own chunk table) before
/// the engine writes a single byte. Every chunk the engine fetches is
/// additionally hash-verified, and the swap is atomic — any failure leaves the
/// running binary intact.
fn maybe_apply_sisr_update() -> io::Result<Option<PathBuf>> {
    let Some(manifest_path) = std::env::var_os("DAEDALUS_SISR_MANIFEST") else {
        return Ok(None);
    };
    let manifest_path = PathBuf::from(manifest_path);
    let remote_bytes = fs::read(&manifest_path)?;
    let remote = daedalus_core::sisr_stage::RemoteManifest::from_bytes(&remote_bytes)?;

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
    let fetcher = daedalus_core::sisr::engine::DirectoryChunkFetcher::new(&chunks_root);

    let current = self_exe()?;
    let store = HealthStore::new(&health_store_dir()?);
    refuse_quarantined_target(&store, &current, &remote.manifest, &fetcher)?;

    let updated = apply_with_rollback_snapshot(&current, &store, |path| {
        daedalus_core::sisr::engine::SisrEngine.apply_update(
            path,
            &remote.manifest,
            &fetcher,
            // Carried into the rebuilt binary's SISR extension so the
            // updated binary keeps its at-rest authenticity (roadmap #45).
            &remote.signature,
        )
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

/// The gate's policy: defaults with `DAEDALUS_HEALTH_TIMEOUT_MS` /
/// `DAEDALUS_HEALTH_MAX_ATTEMPTS` overrides (the test harness uses these to make
/// quarantine immediate).
fn health_policy() -> HealthCheckPolicy {
    let mut policy = HealthCheckPolicy::default();
    if let Some(v) = std::env::var("DAEDALUS_HEALTH_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse().ok())
    {
        policy.timeout_ms = v;
    }
    if let Some(v) = std::env::var("DAEDALUS_HEALTH_MAX_ATTEMPTS")
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
    manifest: &daedalus_core::manifest::DeltaManifest,
    fetcher: &dyn daedalus_core::sisr::engine::ChunkFetcher,
) -> io::Result<()> {
    if !store.has_quarantined()? {
        return Ok(());
    }
    let target = daedalus_core::sisr::engine::SisrEngine
        .target_payload_sha256(current, manifest, fetcher)?;
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
/// `supervised_launch` - supervised launch.
///
/// Description:
///
/// Return: nothing
fn supervised_launch(
    meta: &Metadata,
    rootfs: &Path,
    app_config: &config::AppConfig,
    store: &HealthStore,
    version_id: &str,
    bin_path: &Path,
) -> io::Result<()> {
    let verbose = std::env::var_os("DAEDALUS_VERBOSE").is_some();
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
            eprintln!("[daedalus] health gate: app failed to start: {e}");
        }
        std::process::exit(127);
    }

    match wait_for_child_status(pid, policy.timeout_ms)? {
        ChildStatus::StillRunning => {
            store.confirm(version_id)?;
            let _ = discard_backup(&backup_path_for(bin_path));
            if verbose {
                eprintln!("[daedalus] health gate: version {version_id} healthy");
            }
            exec::install_signal_handler(&[("app".to_string(), pid)]);
            exit(wait_child_exit_code(pid)?);
        }
        ChildStatus::Exited(0) => {
            store.confirm(version_id)?;
            let _ = discard_backup(&backup_path_for(bin_path));
            if verbose {
                eprintln!("[daedalus] health gate: version {version_id} healthy (clean exit)");
            }
            exit(0);
        }
        ChildStatus::Exited(code) | ChildStatus::Signaled(code) => {
            eprintln!("[daedalus] health gate: version {version_id} failed (exit {code})");
            let quarantined = store.record_failure(version_id, policy.max_attempts)?;
            if quarantined {
                eprintln!(
                    "[daedalus] version {version_id} quarantined after {} failed launches",
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
/// `supervised_launch` - supervised launch.
///
/// Description:
///
/// Return: nothing
fn supervised_launch(
    meta: &Metadata,
    rootfs: &Path,
    app_config: &config::AppConfig,
    store: &HealthStore,
    version_id: &str,
    bin_path: &Path,
) -> io::Result<()> {
    let verbose = std::env::var_os("DAEDALUS_VERBOSE").is_some();
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
                eprintln!("[daedalus] health gate: version {version_id} healthy");
            }
            let code = win::wait(&child)?;
            exit(code);
        }
        ChildStatus::Exited(code) => {
            eprintln!("[daedalus] health gate: version {version_id} failed (exit {code})");
            let quarantined = store.record_failure(version_id, policy.max_attempts)?;
            if quarantined {
                eprintln!(
                    "[daedalus] version {version_id} quarantined after {} failed launches",
                    policy.max_attempts
                );
            }
            rollback_to_previous(bin_path, verbose)
        }
        ChildStatus::Signaled(code) => {
            eprintln!("[daedalus] health gate: version {version_id} failed (exit {code})");
            rollback_to_previous(bin_path, verbose)
        }
    }
}

/// Polls `pid` with `WNOHANG` until it exits or `timeout_ms` elapses.
#[cfg(unix)]
/// `wait_for_child_status` - wait for child status.
/// `@pid`: pid
/// `@timeout_ms`: timeout ms
/// `@io`: io
///
/// Description:
///
/// Return: Result containing `io::Result<ChildStatus>`
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
/// `wait_child_exit_code` - wait child exit code.
/// `@pid`: pid
/// `@io`: io
///
/// Description:
///
/// Return: Result containing `io::Result<i32>`
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
/// `decode_exit_status` - decode exit status.
/// `@status`: status code
///
/// Description:
///
/// Return: the `i32`
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
            "[daedalus] rolled back to previous version: {}",
            bin_path.display()
        );
    }
    std::env::remove_var("DAEDALUS_SISR_MANIFEST");
    std::env::remove_var("DAEDALUS_UPDATE_URL");
    exec_again(bin_path)
}

/// Re-execs the current stub binary (a `.daedalus` file) with the original argv.
#[cfg(unix)]
/// `exec_again` - exec again.
/// `@bin_path`: bin path
/// `@io`: io
///
/// Description:
///
/// Return: Result containing `io::Result<()>`
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
/// `exec_again` - exec again.
/// `@bin_path`: bin path
/// `@io`: io
///
/// Description:
///
/// Return: Result containing `io::Result<()>`
fn exec_again(bin_path: &Path) -> io::Result<()> {
    let argv: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let env: std::collections::BTreeMap<String, String> = std::env::vars().collect();
    let child = win::spawn(bin_path, &argv, &env, None, true)?;
    let _ = child.pid;
    exit(0);
}

// ---------------------------------------------------------------------------
// `--daedalus-update` / `--daedalus-version` runtime flags
// ---------------------------------------------------------------------------

/// Intercepts the daedalus-reserved runtime flags and handles them terminally.
///
/// - `--daedalus-version` prints version info on stdout and exits 0.
/// - `--daedalus-update=<URL>` fetches the signed remote manifest and the changed
///   chunks from the update channel, applies the delta atomically, prints
///   reuse/fetch statistics on stderr, and exits 0. A bare `--daedalus-update`
///   falls back to `$DAEDALUS_UPDATE_URL` then the embedded metadata URL.
///
/// Because both paths call `process::exit`, these flags never reach the host
/// app's `argv`. When neither flag is present this is a no-op.
fn handle_runtime_flags(meta: &Metadata) -> io::Result<()> {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();

    if args.iter().any(|a| a == "--daedalus-version") {
        println!("daedalus {} (stub)", env!("CARGO_PKG_VERSION"));
        if let Some(v) = &meta.version {
            println!("app version: {v}");
        }
        exit(0);
    }

    #[cfg(target_os = "linux")]
    if let Some(idx) = args.iter().position(|a| {
        let s = a.to_string_lossy();
        s == "--daedalus-update" || s.starts_with("--daedalus-update=")
    }) {
        let base = resolve_update_url(&args, idx, meta)?;
        remote_update(&base)?;
        exit(0);
    }

    Ok(())
}

#[cfg(target_os = "linux")]
/// Resolves the update channel base URL:
/// `--daedalus-update=<URL>` argument > `$DAEDALUS_UPDATE_URL` > embedded `meta.update_url`.
fn resolve_update_url(
    args: &[std::ffi::OsString],
    idx: usize,
    meta: &Metadata,
) -> io::Result<String> {
    update_url::resolve_update_url(args, idx, meta)
}

#[cfg(target_os = "linux")]
/// Fetches `<base>/manifest` (XBMR), authenticates it against the trusted
/// keys + Merkle root, then streams the changed chunks from `<base>/chunks/<hex>`
/// through the engine. Progress and reuse/fetch stats go to stderr; the
/// process exits after the atomic swap.
fn remote_update(base: &str) -> io::Result<()> {
    eprintln!("[daedalus] update: fetching manifest from {base}/manifest");
    let manifest_bytes = http_get_bytes(&format!("{base}/manifest"))?;
    let remote = daedalus_core::sisr_stage::RemoteManifest::from_bytes(&manifest_bytes)?;

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
    eprintln!("[daedalus] update: manifest verified ({total} chunks)");

    let current = self_exe()?;
    let store = HealthStore::new(&health_store_dir()?);
    let fetcher = HttpChunkFetcher::new(&format!("{base}/chunks"), total);
    refuse_quarantined_target(&store, &current, &remote.manifest, &fetcher)?;

    let updated = apply_with_rollback_snapshot(&current, &store, |path| {
        let (updated, stats) = daedalus_core::sisr::engine::SisrEngine.apply_update_with_stats(
            path,
            &remote.manifest,
            &fetcher,
            // Publisher-signed link embedded in the rebuilt binary (roadmap #45).
            &remote.signature,
        )?;
        eprintln!(
            "[daedalus] update applied: {} chunks reused ({}), {} chunks fetched ({}), total {total}",
            stats.reused_chunks,
            human_bytes(stats.reused_bytes),
            stats.fetched_chunks,
            human_bytes(stats.fetched_bytes),
        );
        Ok(updated)
    })?;
    eprintln!("[daedalus] updated binary: {}", updated.display());
    Ok(())
}

/// [`ChunkFetcher`] pulling chunks from `<base>/<64-hex-sha256>` over HTTPS.
///
/// Content-addressability is the security anchor: every chunk the engine
/// writes must SHA-256 to its manifest entry, so the transport cannot smuggle
/// a wrong chunk in. The fetcher only counts + reports progress.
#[cfg(target_os = "linux")]
struct HttpChunkFetcher {
    base: String,
    total: usize,
    done: std::cell::Cell<usize>,
    bytes: std::cell::Cell<u64>,
}

#[cfg(target_os = "linux")]
impl HttpChunkFetcher {
    /// `new` - new.
    /// `@base`: base
    /// `@total`: total
    ///
    /// Description:
    ///
    /// Return: the `Self`
    fn new(base: &str, total: usize) -> Self {
        Self {
            base: base.to_string(),
            total,
            done: std::cell::Cell::new(0),
            bytes: std::cell::Cell::new(0),
        }
    }
}

#[cfg(target_os = "linux")]
impl daedalus_core::sisr::engine::ChunkFetcher for HttpChunkFetcher {
    /// `fetch` - fetch.
    /// `@hash`: hash value
    /// `@length`: length
    /// `@io`: io
    ///
    /// Description:
    ///
    /// Return: Result containing `io::Result<Vec<u8>`>
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
            "[daedalus]   fetched chunk {done}/{} ({} bytes)",
            self.total,
            bytes.len()
        );
        Ok(bytes)
    }

    /// `bytes_fetched` - bytes fetched.
    ///
    /// Description:
    ///
    /// Return: the `u64`
    fn bytes_fetched(&self) -> u64 {
        self.bytes.get()
    }
}

#[cfg(target_os = "linux")]
/// Integer duration in milliseconds from the env, falling back to `default_ms`
/// when unset or unparsable.
fn env_timeout_ms(name: &str, default_ms: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default_ms)
}

/// Minimal HTTPS GET returning the raw response body. Timeouts are tunable via
/// `DAEDALUS_HTTP_TIMEOUT_CONNECT`, `DAEDALUS_HTTP_TIMEOUT_RESPONSE`, and
/// `DAEDALUS_HTTP_TIMEOUT_BODY` (milliseconds; defaults 10s / 30s / 30s).
/// `DAEDALUS_HTTP_MAX_RESPONSE` sets the max response body in bytes (default 64 MiB).
///
/// Only caller-verified content is consumed (signed manifest, hash-checked
/// chunks), so the transport is a convenience — never a trust anchor.
#[cfg(target_os = "linux")]
/// `http_get_bytes` - http get bytes.
/// `@url`: URL
/// `@io`: io
///
/// Description:
///
/// Return: Result containing `io::Result<Vec<u8>`>
fn http_get_bytes(url: &str) -> io::Result<Vec<u8>> {
    const DEFAULT_MAX: u64 = 64 * 1024 * 1024; // 64 MiB
    let ms = |name, default| std::time::Duration::from_millis(env_timeout_ms(name, default));
    let max_bytes = env_timeout_ms("DAEDALUS_HTTP_MAX_RESPONSE", DEFAULT_MAX);
    let resp = ureq::get(url)
        .config()
        .timeout_connect(Some(ms("DAEDALUS_HTTP_TIMEOUT_CONNECT", 10_000)))
        .timeout_recv_response(Some(ms("DAEDALUS_HTTP_TIMEOUT_RESPONSE", 30_000)))
        .timeout_recv_body(Some(ms("DAEDALUS_HTTP_TIMEOUT_BODY", 30_000)))
        .build()
        .call()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("GET {url}: {e}")))?;
    let mut body = resp.into_body();
    let reader = body.as_reader();
    let mut buf = Vec::new();
    reader
        .take(max_bytes)
        .read_to_end(&mut buf)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("GET {url}: {e}")))?;
    if buf.len() as u64 >= max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("GET {url}: response exceeds {max_bytes} bytes"),
        ));
    }
    Ok(buf)
}

#[allow(dead_code)]
/// `human_bytes` - human bytes.
/// `@bytes`: bytes
///
/// Description:
///
/// Return: the `resulting` string
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
/// Linux: `$XDG_CACHE_HOME/daedalus` or `~/.cache/daedalus`.
/// macOS: `$XDG_CACHE_HOME/daedalus` if set, else `~/Library/Caches/daedalus`
/// (`dirs::cache_dir()`).
/// Windows: `%LOCALAPPDATA%\daedalus`.
fn cache_dir() -> io::Result<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(xdg).join("daedalus"));
    }
    #[cfg(target_os = "windows")]
    {
        let local = std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA not set"))?;
        Ok(PathBuf::from(local).join("daedalus"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let dir = dirs::cache_dir().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "no cache directory available")
        })?;
        Ok(dir.join("daedalus"))
    }
}

/// Garbage-collect the extracted rootfs cache, keeping at most `max_entries`
/// directories (LRU by `.ready` mtime). Called before a cold extraction so
/// the cache does not grow without bound.
///
/// The extraction cache lives under `cache_dir()/{hash}/rootfs/`. Each entry
/// has a `.ready` marker whose mtime is updated on every warm hit, giving a
/// cheap LRU signal without extra metadata files.
/// Advisory cross-process lock for cache GC, taken non-blocking so a GC that
/// finds another GC in progress simply skips this run. `flock` is released by
/// the kernel when the process exits (even a crash), so no stale-lock cleanup
/// is needed.
struct GcLock {
    /// Held for its lifetime: keeping the fd open is what holds the flock.
    _file: File,
}

impl GcLock {
    /// `acquire` - acquire.
    /// `@base`: base
    /// `@io`: io
    ///
    /// Description:
    ///
    /// Return: Result containing `io::Result<Option<GcLock>`>
    fn acquire(base: &Path) -> io::Result<Option<GcLock>> {
        fs::create_dir_all(base)?;
        let file = fs::File::create(base.join(".gc.lock"))?;
        #[cfg(unix)]
        {
            // SAFETY: flock(2) is advisory and the fd is valid + owned. LOCK_NB
            // makes the call fail with EWOULDBLOCK if another process holds it.
            let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if rc != 0 {
                return Ok(None);
            }
        }
        Ok(Some(GcLock { _file: file }))
    }
}

/// Evict the oldest completed extraction caches beyond `max_entries`.
///
/// Serialized across processes via `GcLock`. Only entries carrying a `.ready`
/// marker are eviction candidates: a concurrent extraction has no marker yet,
/// so it can never be picked as "oldest" and deleted mid-extract.
fn gc_extraction_cache(max_entries: usize) -> io::Result<()> {
    let base = cache_dir()?;
    if GcLock::acquire(&base)?.is_none() {
        return Ok(());
    }
    let mut entries: Vec<_> = match fs::read_dir(&base) {
        Ok(iter) => iter.filter_map(Result::ok).collect(),
        Err(_) => return Ok(()),
    };
    entries.retain(|e| e.path().join(".ready").is_file());
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

/// `extract_atomic` - extract atomic.
/// `@blobs`: blobs
/// `@cache_root`: cache root
/// `@io`: io
///
/// Description:
///
/// Return: Result containing `io::Result<()>`
fn extract_atomic(blobs: &[&[u8]], cache_root: &Path) -> io::Result<()> {
    extraction::extract_atomic(blobs, cache_root)
}

/// `extract_squashfs_atomic` - extract squashfs atomic.
/// `@blobs`: blobs
/// `@cache_root`: cache root
/// `@io`: io
///
/// Description:
///
/// Return: Result containing `io::Result<()>`
fn extract_squashfs_atomic(blobs: &[&[u8]], cache_root: &Path) -> io::Result<()> {
    extraction::extract_squashfs_atomic(blobs, cache_root)
}

// ---------------------------------------------------------------------------
// Seccomp BPF denylist
// ---------------------------------------------------------------------------
#[cfg(target_os = "linux")]
/// `pivot_root_into` - pivot root into.
/// `@rootfs`: rootfs
/// `@io`: io
///
/// Description:
///
/// Return: Result containing `io::Result<()>`
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
/// `cstr` - cstr.
/// `@bytes`: bytes
/// `@io`: io
///
/// Description:
///
/// Return: Result containing `io::Result<CString>`
fn cstr(bytes: &[u8]) -> io::Result<CString> {
    CString::new(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "path contains null byte"))
}

#[cfg(unix)]
/// `to_ptr_vec` - to ptr vec.
/// `@v`: v
/// `@core`: core
/// `@ffi`: ffi
///
/// Description:
///
/// Return: vector of `Vec<*const core::ffi::c_char>`
fn to_ptr_vec(v: &[CString]) -> Vec<*const core::ffi::c_char> {
    let mut ptrs: Vec<*const core::ffi::c_char> = v.iter().map(|c| c.as_ptr()).collect();
    ptrs.push(std::ptr::null());
    ptrs
}

/// `nanos` - nanos.
///
/// Description:
///
/// Return: the `u128`
pub fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// Acquire an exclusive advisory lock (flock(2)) on `f`.
#[cfg(unix)]
/// `flock_exclusive` - flock exclusive.
/// `@f`: f
/// `@io`: io
///
/// Description:
///
/// Return: Result containing `io::Result<()>`
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
/// `flock_exclusive` - flock exclusive.
/// `@_f`:  f
/// `@io`: io
///
/// Description:
///
/// Return: Result containing `io::Result<()>`
fn flock_exclusive(_f: &File) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
extern "C" {
    #[link_name = "execvp"]
    /// `libc_execvp` - libc execvp.
    /// `@path`: file or directory path
    /// `@core`: core
    /// `@ffi`: ffi
    /// `@argv`: argv
    /// `@core`: core
    /// `@ffi`: ffi
    ///
    /// Description:
    ///
    /// Return: the `i32`;
    fn libc_execvp(path: *const core::ffi::c_char, argv: *const *const core::ffi::c_char) -> i32;
    #[link_name = "execve"]
    /// `libc_execve` - libc execve.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn libc_execve(
        path: *const core::ffi::c_char,
        argv: *const *const core::ffi::c_char,
        envp: *const *const core::ffi::c_char,
    ) -> i32;
    #[link_name = "flock"]
    /// `libc_flock` - libc flock.
    /// `@fd`: fd
    /// `@operation`: operation
    ///
    /// Description:
    ///
    /// Return: the `i32`;
    fn libc_flock(fd: i32, operation: i32) -> i32;
}

/// `err` - err.
/// `@msg`: message
/// `@io`: io
///
/// Description:
///
/// Return: the `std::io::Error`
fn err(msg: impl AsRef<str>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.as_ref())
}

// ---------------------------------------------------------------------------
// Health check HTTP server
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// `human_bytes_formats_all_scales` - human bytes formats all scales.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn human_bytes_formats_all_scales() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(1536), "1.5 KiB");
        assert_eq!(human_bytes(1024 * 1024), "1.0 MiB");
        assert_eq!(human_bytes((1024 * 1024) + (512 * 1024)), "1.5 MiB");
    }

    #[test]
    /// `env_timeout_ms_reads_int_and_falls_back` - env timeout ms reads int and falls back.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn env_timeout_ms_reads_int_and_falls_back() {
        std::env::set_var("DAEDALUS_HTTP_TIMEOUT_TEST", "2500");
        assert_eq!(env_timeout_ms("DAEDALUS_HTTP_TIMEOUT_TEST", 10_000), 2500);
        std::env::remove_var("DAEDALUS_HTTP_TIMEOUT_TEST");
        assert_eq!(env_timeout_ms("DAEDALUS_HTTP_TIMEOUT_TEST", 10_000), 10_000);
        std::env::set_var("DAEDALUS_HTTP_TIMEOUT_TEST", "garbage");
        assert_eq!(env_timeout_ms("DAEDALUS_HTTP_TIMEOUT_TEST", 10_000), 10_000);
        std::env::remove_var("DAEDALUS_HTTP_TIMEOUT_TEST");
    }

    #[test]
    /// `gc_extraction_cache_never_evicts_in_progress_extraction` - gc extraction cache never evicts in progress extraction.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn gc_extraction_cache_never_evicts_in_progress_extraction() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CACHE_HOME", tmp.path());
        // Two completed caches (with .ready) and one in-progress extraction
        // (rootfs present, no .ready marker yet).
        for name in ["aaa", "bbb", "ccc"] {
            let root = tmp.path().join("daedalus").join(name);
            fs::create_dir_all(root.join("rootfs")).unwrap();
        }
        fs::write(tmp.path().join("daedalus/aaa/.ready"), b"").unwrap();
        fs::write(tmp.path().join("daedalus/bbb/.ready"), b"").unwrap();

        gc_extraction_cache(1).unwrap();

        let survivors: Vec<String> = fs::read_dir(tmp.path().join("daedalus"))
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n != ".gc.lock")
            .collect();
        assert_eq!(
            survivors.len(),
            2,
            "only one completed cache may be evicted"
        );
        assert!(
            survivors.contains(&"ccc".to_string()),
            "in-progress extraction 'ccc' must never be evicted"
        );
        std::env::remove_var("XDG_CACHE_HOME");
    }

    /// Builds a v2 layout `[stub][payload][meta][tail][84-byte footer]` for
    /// the downgrade-detection tests.
    fn build_v2_bytes(payload: &[u8], meta: &[u8], tail: &[u8]) -> Vec<u8> {
        let stub = [0u8; 64];
        let footer = Footer {
            format_version: 2,
            arch: 0x01,
            flags: 0,
            payload_offset: stub.len() as u64,
            payload_csize: payload.len() as u64,
            payload_usize: 0,
            payload_sha256: [0u8; 32],
            meta_offset: (stub.len() + payload.len()) as u64,
            meta_size: meta.len() as u64,
            sig_offset: 0,
        };
        let mut data = stub.to_vec();
        data.extend_from_slice(payload);
        data.extend_from_slice(meta);
        data.extend_from_slice(tail);
        data.extend_from_slice(&footer.pack());
        data
    }

    #[test]
    /// `downgrade_reject_detects_leftover_sig_block` - downgrade reject detects leftover sig block.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn downgrade_reject_detects_leftover_sig_block() {
        let mut leftover = Vec::new();
        leftover.extend_from_slice(&(format::SIG_LEN as u32).to_le_bytes());
        leftover.extend_from_slice(&[0xAAu8; format::SIG_LEN]);
        // Lazy downgrade keeps the 8-byte sig_offset prefix between the sig
        // block and the rewritten v2 core.
        let mut with_prefix = Vec::new();
        with_prefix.extend_from_slice(&leftover);
        with_prefix.extend_from_slice(&[0u8; 8]);
        let data = build_v2_bytes(b"payload", b"{}", &with_prefix);
        let footer = Footer::read_from(&mut std::io::Cursor::new(&data)).unwrap();
        let result = reject_downgraded_sig_block(&mut std::io::Cursor::new(data), &footer);
        assert!(
            result.is_err(),
            "leftover sig block (prefix kept) must be rejected"
        );
        // Downgrade that also stripped the prefix.
        let data = build_v2_bytes(b"payload", b"{}", &leftover);
        let footer = Footer::read_from(&mut std::io::Cursor::new(&data)).unwrap();
        let result = reject_downgraded_sig_block(&mut std::io::Cursor::new(data), &footer);
        assert!(
            result.is_err(),
            "leftover sig block (prefix stripped) must be rejected"
        );
    }

    #[test]
    /// `downgrade_reject_accepts_clean_v2_layout` - downgrade reject accepts clean v2 layout.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn downgrade_reject_accepts_clean_v2_layout() {
        let data = build_v2_bytes(b"payload", b"{}", &[]);
        let footer = Footer::read_from(&mut std::io::Cursor::new(&data)).unwrap();
        let result = reject_downgraded_sig_block(&mut std::io::Cursor::new(data), &footer);
        assert!(result.is_ok());
    }

    #[test]
    /// `downgrade_reject_ignores_unrelated_gaps` - downgrade reject ignores unrelated gaps.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn downgrade_reject_ignores_unrelated_gaps() {
        let data = build_v2_bytes(b"payload", b"{}", &[0x00; 20]);
        let footer = Footer::read_from(&mut std::io::Cursor::new(&data)).unwrap();
        let result = reject_downgraded_sig_block(&mut std::io::Cursor::new(data), &footer);
        assert!(result.is_ok());

        let data = build_v2_bytes(b"payload", b"{}", &[0x00; format::SIG_BLOCK_SIZE]);
        let footer = Footer::read_from(&mut std::io::Cursor::new(&data)).unwrap();
        let result = reject_downgraded_sig_block(&mut std::io::Cursor::new(data), &footer);
        assert!(
            result.is_ok(),
            "a sig-sized gap without size field 64 is not a signature block"
        );
    }

    #[test]
    /// `layer_manifest_roundtrip_from_metadata` - layer manifest roundtrip from metadata.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn layer_manifest_roundtrip_from_metadata() {
        let rt = daedalus_core::layer::RuntimeLayer {
            name: "python3".into(),
            interpreter: "python3".into(),
            entrypoint: vec!["python3 /app/main.py".into()],
            version: Some("3.11".into()),
            env: vec![("FOO".into(), "bar".into())],
            capabilities: vec![daedalus_core::layer::Capability::ReadFile],
        };
        let cfg = daedalus_core::layer::ConfigLayer {
            name: "app-config".into(),
            format: "toml".to_string(),
            data: serde_json::json!({ "port": 8080 }),
        };
        let meta = Metadata {
            name: "test-app".into(),
            version: None,
            runtime: "python3".into(),
            entrypoint: vec![],
            env: std::collections::BTreeMap::new(),
            cwd: None,
            isolation: 0,
            seccomp: false,
            landlock: false,
            gui: false,
            cpu_limit: None,
            memory_limit_mb: None,
            pid_limit: None,
            services: vec![],
            payload_format: "zstd+tar".into(),
            health_check: None,
            update_url: None,
            layers: vec![
                daedalus_core::layer::SerializableLayer::Runtime(rt),
                daedalus_core::layer::SerializableLayer::Config(cfg),
            ],
            entrypoint_layer: Some("python3".into()),
            hooks: None,
            encryption: None,
        };
        let manifest = LayerManifest::from_metadata(&meta);
        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.layers.len(), 2);
        assert_eq!(manifest.layers[0].name, "python3");
        assert_eq!(manifest.layers[0].kind, "runtime");
        assert_eq!(manifest.layers[0].rootfs_path, Some("/app".to_string()));
        assert_eq!(manifest.layers[1].name, "app-config");
        assert_eq!(manifest.layers[1].kind, "config");
        assert_eq!(manifest.layers[1].rootfs_path, Some("/app".to_string()));
        // Round-trip through JSON
        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: LayerManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, manifest);
    }

    #[test]
    /// `write_layer_manifest_creates_file` - write layer manifest creates file.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn write_layer_manifest_creates_file() {
        let tmp = tempfile::tempdir().unwrap();
        let meta = Metadata {
            name: "test".into(),
            version: None,
            runtime: "python3".into(),
            entrypoint: vec![],
            env: std::collections::BTreeMap::new(),
            cwd: None,
            isolation: 0,
            seccomp: false,
            landlock: false,
            gui: false,
            cpu_limit: None,
            memory_limit_mb: None,
            pid_limit: None,
            services: vec![],
            payload_format: "zstd+tar".into(),
            health_check: None,
            update_url: None,
            layers: vec![],
            entrypoint_layer: None,
            hooks: None,
            encryption: None,
        };
        write_layer_manifest(tmp.path(), &meta).unwrap();
        assert!(tmp.path().join(".daedalus-layers.json").is_file());
        // Should be loadable back
        let loaded = load_layer_manifest(tmp.path()).unwrap();
        assert_eq!(loaded.version, 1);
        assert!(loaded.layers.is_empty());
    }
}
