//! .xbin binary assembly — writes the final executable.
//!
//! Classic layout: [stub][payload][metadata][footer]. With the `SISR` stage
//! enabled the layout becomes
//! [stub][payload][manifest][SisrFooterExt][metadata][footer] and a signed
//! remote manifest is written next to the binary.

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::format::{self, Footer, CRYPTO_AES_256_GCM};
use crate::metadata::BunFeatures;
use crate::sisr_header::{SisrFooterExt, SISR_VERSION};
use crate::sisr_stage::{self, RemoteManifest, SisrBuildConfig};

/// Determine the format version based on build options.
pub fn fmt_version(squashfs: bool, encrypt: bool, signed: bool) -> u8 {
    if squashfs {
        5
    } else if encrypt {
        4
    } else if signed {
        3
    } else {
        2
    }
}

/// Determine architecture from target string or host.
/// Accepts short forms (`aarch64`) and Rust triples (`aarch64-apple-darwin`).
pub fn resolve_arch(target_arch: Option<&str>) -> u8 {
    let arch = target_arch
        .map(|t| t.split('-').next().unwrap_or(t))
        .unwrap_or_else(|| std::env::consts::ARCH);
    match arch {
        "aarch64" | "arm64" => format::ARCH_AARCH64,
        _ => format::ARCH_X86_64,
    }
}

/// Build the metadata JSON bytes.
#[allow(clippy::too_many_arguments)]
pub fn build_meta_json(
    name: &str,
    runtime: &str,
    isolation: u32,
    entrypoint: &[String],
    env: &[(String, String)],
    layers: &[serde_json::Value],
    options: &MetaOptions,
    bun_features: &BunFeatures,
) -> std::io::Result<Vec<u8>> {
    let mut meta = serde_json::json!({
        "name": name,
        "xbin_version": env!("CARGO_PKG_VERSION"),
        "created": chrono_now(),
        "runtime": runtime,
        "isolation": isolation,
        "entrypoint": entrypoint,
        "env": env_map(env),
        "layers": layers,
        "cwd": "/app",
    });

    if let Some(v) = &options.version {
        meta["version"] = serde_json::Value::String(v.clone());
    }
    if let Some(a) = &options.author {
        meta["author"] = serde_json::Value::String(a.clone());
    }
    if let Some(d) = &options.description {
        meta["description"] = serde_json::Value::String(d.clone());
    }
    if let Some(l) = &options.license {
        meta["license"] = serde_json::Value::String(l.clone());
    }
    if let Some(pf) = &options.payload_format {
        meta["payload_format"] = serde_json::Value::String(pf.clone());
    }
    if options.seccomp {
        meta["seccomp"] = serde_json::Value::Bool(true);
    }
    if options.landlock {
        meta["landlock"] = serde_json::Value::Bool(true);
    }
    if let Some(c) = &options.app_hash {
        meta["app_hash"] = serde_json::Value::String(c.clone());
    }
    if let Some(h) = &options.rt_deps_hash {
        meta["rt_deps_hash"] = serde_json::Value::String(h.clone());
    }
    if let Some(u) = &options.update_url {
        meta["update_url"] = serde_json::Value::String(u.clone());
    }

    if bun_features.health_check.enabled {
        meta["health_check"] = serde_json::to_value(&bun_features.health_check)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    }

    if bun_features.embedded_runtime.interpreter.is_some() {
        meta["embedded_runtime"] = serde_json::to_value(&bun_features.embedded_runtime)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    }

    if bun_features.wasm.enabled {
        meta["wasm"] = serde_json::to_value(&bun_features.wasm)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    }

    if bun_features.build_cache.enabled {
        meta["build_cache"] = serde_json::to_value(&bun_features.build_cache)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    }

    if !bun_features.cross_compile_targets.is_empty() {
        meta["cross_compile_targets"] = serde_json::Value::Array(
            bun_features
                .cross_compile_targets
                .iter()
                .map(|t| serde_json::Value::String(t.clone()))
                .collect(),
        );
    }

    serde_json::to_vec(&meta).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Options for metadata construction.
pub struct MetaOptions {
    pub version: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub payload_format: Option<String>,
    pub seccomp: bool,
    pub landlock: bool,
    pub app_hash: Option<String>,
    pub rt_deps_hash: Option<String>,
    /// Base URL of the SISR update channel (`{url}/manifest`, `{url}/chunks/<hex>`).
    pub update_url: Option<String>,
}

/// Assemble a .xbin file from its components (without signing).
///
/// Writes: [stub][payload][metadata][footer]
/// Returns the total file size.
pub fn assemble_xbin(
    out_path: &Path,
    stub_bytes: &[u8],
    payload: &[u8],
    meta_bytes: &[u8],
    encrypt: bool,
    squashfs: bool,
    target_arch: Option<&str>,
) -> std::io::Result<u64> {
    assemble_xbin_with_sisr(
        out_path,
        stub_bytes,
        payload,
        meta_bytes,
        encrypt,
        squashfs,
        target_arch,
        &SisrBuildConfig::disabled(),
    )
}

/// Assemble a .xbin file, optionally with a `SISR` section and remote manifest.
///
/// With `config.enabled` false the layout is strictly identical to
/// [`assemble_xbin`]. With `SISR` enabled the payload is content-chunked, a
/// Merkle root is computed, and the layout becomes:
///
/// `[stub][payload][manifest][metadata][SisrFooterExt][footer]`
///
/// The footer gains `FLAG_SISR`, and the signed remote manifest is written to
/// `<out_path>.manifest` next to the binary. Returns the total file size.
// Params mirror `assemble_xbin` plus the `SISR` config; a shared struct would
// churn both public entry points for a single new optional stage.
#[allow(clippy::too_many_arguments)]
pub fn assemble_xbin_with_sisr(
    out_path: &Path,
    stub_bytes: &[u8],
    payload: &[u8],
    meta_bytes: &[u8],
    encrypt: bool,
    squashfs: bool,
    target_arch: Option<&str>,
    config: &SisrBuildConfig,
) -> std::io::Result<u64> {
    let fmt_ver = fmt_version(squashfs, encrypt, false);
    let arch = resolve_arch(target_arch);
    let payload_offset = stub_bytes.len() as u64;

    let sisr = if config.enabled {
        let artifacts = sisr_stage::build_artifacts(payload, config)?;
        let manifest_offset = payload_offset + payload.len() as u64 + meta_bytes.len() as u64;
        let ext = SisrFooterExt {
            sisr_version: SISR_VERSION,
            chunk_table_offset: manifest_offset,
            chunk_table_len: u32::try_from(artifacts.manifest_bytes.len())
                .map_err(|_| io_err("SISR manifest exceeds capacity"))?,
            merkle_root: artifacts.merkle_root,
            signature: artifacts.signature,
        };
        Some((artifacts, ext))
    } else {
        None
    };

    let meta_offset = payload_offset + payload.len() as u64;

    let body_hash = sha2_hash(payload, meta_bytes);

    let footer = Footer {
        format_version: fmt_ver,
        arch,
        flags: if sisr.is_some() { format::FLAG_SISR } else { 0 },
        payload_offset,
        payload_csize: payload.len() as u64,
        payload_usize: if encrypt { CRYPTO_AES_256_GCM } else { 0 },
        payload_sha256: body_hash,
        meta_offset,
        meta_size: meta_bytes.len() as u64,
        sig_offset: 0,
    };

    let mut f = fs::File::create(out_path)?;
    f.write_all(stub_bytes)?;
    f.write_all(payload)?;
    f.write_all(meta_bytes)?;
    if let Some((artifacts, ext)) = &sisr {
        f.write_all(&artifacts.manifest_bytes)?;
        f.write_all(&ext.pack())?;
    }
    f.write_all(&footer.pack())?;
    f.flush()?;

    // Set executable permission
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(out_path, fs::Permissions::from_mode(0o755))?;
    }

    if let Some((artifacts, _)) = &sisr {
        let remote = RemoteManifest {
            merkle_root: artifacts.merkle_root,
            signature: artifacts.signature,
            manifest: artifacts.manifest.clone(),
        };
        let mut manifest_path = out_path.to_path_buf();
        manifest_path.set_extension("xbin.manifest");
        fs::write(manifest_path, remote.to_bytes())?;
    }

    Ok(std::fs::metadata(out_path)?.len())
}

fn io_err(msg: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg)
}

/// SHA-256(payload || `meta_bytes`) — the integrity hash.
fn sha2_hash(payload: &[u8], meta: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(payload);
    hasher.update(meta);
    hasher.finalize().into()
}

/// ISO 8601 timestamp (UTC).
fn chrono_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86400;
    let rem = secs % 86400;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;
    let (year, month, day) = unix_days_to_date(days);
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}Z")
}

fn unix_days_to_date(mut days: u64) -> (u32, u32, u32) {
    let mut year = 1970u32;
    let mut month = 1u32;
    let mut day = 1u32;

    fn is_leap(y: u32) -> bool {
        y.is_multiple_of(4) && !y.is_multiple_of(100) || y.is_multiple_of(400)
    }
    fn days_in_year(y: u32) -> u64 {
        if is_leap(y) {
            366
        } else {
            365
        }
    }
    fn days_in_month(y: u32, m: u32) -> u64 {
        match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if is_leap(y) {
                    29
                } else {
                    28
                }
            }
            _ => 0,
        }
    }

    while days > 0 {
        let diy = days_in_year(year);
        if days >= diy {
            days -= diy;
            year += 1;
        } else {
            while days > 0 {
                let dim = days_in_month(year, month);
                if days >= dim {
                    days -= dim;
                    month += 1;
                } else {
                    day = day.saturating_add(days as u32);
                    break;
                }
            }
            break;
        }
    }
    (year, month, day)
}

fn env_map(env: &[(String, String)]) -> serde_json::Value {
    let map: serde_json::Map<String, serde_json::Value> = env
        .iter()
        .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
        .collect();
    serde_json::Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_version_squashfs_is_5() {
        assert_eq!(fmt_version(true, false, false), 5);
    }

    #[test]
    fn fmt_version_encrypt_is_4() {
        assert_eq!(fmt_version(false, true, false), 4);
    }

    #[test]
    fn fmt_version_signed_is_3() {
        assert_eq!(fmt_version(false, false, true), 3);
    }

    #[test]
    fn fmt_version_default_is_2() {
        assert_eq!(fmt_version(false, false, false), 2);
    }

    #[test]
    fn resolve_arch_aarch64() {
        assert_eq!(resolve_arch(Some("aarch64")), format::ARCH_AARCH64);
    }

    #[test]
    fn resolve_arch_x86_64() {
        assert_eq!(resolve_arch(Some("x86_64")), format::ARCH_X86_64);
    }

    #[test]
    fn assemble_creates_valid_file() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("test.xbin");
        let stub = b"STUB_DATA_HERE";
        let payload = b"PAYLOAD_DATA";
        let meta = br#"{"name":"test"}"#;

        let size = assemble_xbin(&out, stub, payload, meta, false, false, None).unwrap();
        assert!(size > 0);

        let data = fs::read(&out).unwrap();
        let s = stub.len();
        let p = payload.len();
        assert_eq!(&data[..s], stub);
        assert_eq!(&data[s..s + p], payload);
        assert_eq!(&data[s + p..s + p + meta.len()], meta);
    }

    #[test]
    fn build_meta_json_produces_valid_json() {
        let opts = MetaOptions {
            version: Some("1.0".into()),
            author: None,
            description: None,
            license: None,
            payload_format: None,
            seccomp: false,
            landlock: false,
            app_hash: None,
            rt_deps_hash: None,
            update_url: None,
        };
        let bun_features = BunFeatures::default();
        let json = build_meta_json(
            "myapp",
            "python",
            0,
            &["python3".into(), "app.py".into()],
            &[("PORT".into(), "8000".into())],
            &[],
            &opts,
            &bun_features,
        )
        .expect("meta serialization failed");
        let parsed: serde_json::Value = serde_json::from_slice(&json).unwrap();
        assert_eq!(parsed["name"], "myapp");
        assert_eq!(parsed["runtime"], "python");
        assert_eq!(parsed["version"], "1.0");
    }

    #[test]
    fn unix_days_to_date_epoch_is_1970_01_01() {
        let (y, m, d) = unix_days_to_date(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn unix_days_to_date_one_year_later() {
        let (y, m, d) = unix_days_to_date(365);
        assert_eq!((y, m, d), (1971, 1, 1));
    }

    #[test]
    fn unix_days_to_date_leap_day_1972() {
        let (y, m, d) = unix_days_to_date(365 + 365);
        assert_eq!((y, m, d), (1972, 1, 1));
        let (y2, m2, d2) = unix_days_to_date(365 + 365 + 366);
        assert_eq!((y2, m2, d2), (1973, 1, 1));
    }

    #[test]
    fn disabled_sisr_is_byte_identical_to_classic() {
        let tmp = tempfile::tempdir().unwrap();
        let stub = b"STUB_DATA_HERE";
        let payload = b"PAYLOAD_DATA_PAYLOAD_DATA";
        let meta = br#"{"name":"test"}"#;

        let classic = tmp.path().join("classic.xbin");
        let size = assemble_xbin(&classic, stub, payload, meta, false, false, None).unwrap();
        let with_sisr = tmp.path().join("with_sisr.xbin");
        let size2 = assemble_xbin_with_sisr(
            &with_sisr,
            stub,
            payload,
            meta,
            false,
            false,
            None,
            &SisrBuildConfig::disabled(),
        )
        .unwrap();
        assert_eq!(size, size2);
        assert_eq!(fs::read(&classic).unwrap(), fs::read(&with_sisr).unwrap());
    }

    #[test]
    fn enabled_sisr_writes_section_and_remote_manifest() {
        use crate::format::Footer;
        use crate::sisr_header::read_sisr;
        use ed25519_dalek::Signer;
        use std::io::Cursor;

        let tmp = tempfile::tempdir().unwrap();
        let stub = b"STUB_DATA_HERE";
        let payload = b"PAYLOAD_DATA_PAYLOAD_DATA_PAYLOAD_DATA";
        let meta = br#"{"name":"test"}"#;
        let out = tmp.path().join("app.xbin");

        let key = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        let config = SisrBuildConfig {
            enabled: true,
            chunk_target_size: 8192,
            signing_key: Some(key.clone()),
        };
        assemble_xbin_with_sisr(&out, stub, payload, meta, false, false, None, &config).unwrap();

        let data = fs::read(&out).unwrap();
        let footer = Footer::read_from(&mut Cursor::new(&data)).unwrap();
        assert_eq!(footer.flags & format::FLAG_SISR, format::FLAG_SISR);

        let (parsed_ext, manifest) = read_sisr(&mut Cursor::new(&data)).unwrap().unwrap();
        assert_eq!(manifest.payload_len, payload.len() as u64);
        assert_eq!(
            crate::sisr_stage::merkle_root_of(&manifest),
            parsed_ext.merkle_root
        );

        let mut msg = Vec::with_capacity(32 + manifest.encoded_len());
        msg.extend_from_slice(&parsed_ext.merkle_root);
        msg.extend_from_slice(&manifest.serialize());
        assert_eq!(
            key.sign(&msg).to_bytes(),
            parsed_ext.signature,
            "embedded signature must match key over merkle||manifest"
        );

        let remote_path = tmp.path().join("app.xbin.manifest");
        assert!(remote_path.exists());
        let remote = RemoteManifest::from_bytes(&fs::read(&remote_path).unwrap()).unwrap();
        assert!(remote.verify_signature(&key.verifying_key()));
        assert!(remote.verify_merkle());
        assert_eq!(remote.manifest, manifest);
    }
}
