//! .xbin binary assembly — writes the final executable.
//!
//! Creates the .xbin file layout: [stub][payload][metadata][footer].
//! Signing is handled separately (requires Ed25519 key + xbin-crypto).

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::format::{self, Footer, CRYPTO_AES_256_GCM};

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
pub fn resolve_arch(target_arch: Option<&str>) -> u8 {
    match target_arch {
        Some("aarch64") => format::ARCH_AARCH64,
        _ => {
            // Default to x86_64 — aarch64 detection happens at compile time
            // or is passed explicitly via --target
            if cfg!(target_arch = "aarch64") {
                format::ARCH_AARCH64
            } else {
                format::ARCH_X86_64
            }
        }
    }
}

/// Build the metadata JSON bytes.
pub fn build_meta_json(
    name: &str,
    runtime: &str,
    isolation: u32,
    entrypoint: &[String],
    env: &[(String, String)],
    layers: &[serde_json::Value],
    options: &MetaOptions,
) -> Vec<u8> {
    let mut meta = serde_json::json!({
        "name": name,
        "xbin_version": env!("CARGO_PKG_VERSION"),
        "created": chrono_now(),
        "runtime": runtime,
        "isolation": isolation,
        "entrypoint": entrypoint,
        "env": env_map(env),
        "layers": layers,
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
    if let Some(c) = &options.app_hash {
        meta["app_hash"] = serde_json::Value::String(c.clone());
    }
    if let Some(h) = &options.rt_deps_hash {
        meta["rt_deps_hash"] = serde_json::Value::String(h.clone());
    }

    serde_json::to_vec(&meta).unwrap_or_default()
}

/// Options for metadata construction.
pub struct MetaOptions {
    pub version: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub payload_format: Option<String>,
    pub seccomp: bool,
    pub app_hash: Option<String>,
    pub rt_deps_hash: Option<String>,
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
    let fmt_ver = fmt_version(squashfs, encrypt, false);
    let arch = resolve_arch(target_arch);
    let payload_offset = stub_bytes.len() as u64;
    let meta_offset = payload_offset + payload.len() as u64;

    let body_hash = sha2_hash(payload, meta_bytes);

    let footer = Footer {
        format_version: fmt_ver,
        arch,
        flags: 0,
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
    f.write_all(&footer.pack())?;
    f.flush()?;

    // Set executable permission
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(out_path, fs::Permissions::from_mode(0o755))?;
    }

    Ok(std::fs::metadata(out_path)?.len())
}

/// SHA-256(payload || `meta_bytes`) — the integrity hash.
fn sha2_hash(payload: &[u8], meta: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(payload);
    hasher.update(meta);
    hasher.finalize().into()
}

/// Simple ISO 8601 timestamp (UTC). Avoids chrono dependency.
fn chrono_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86400;
    let remaining = secs % 86400;
    let h = remaining / 3600;
    let m = (remaining % 3600) / 60;
    let s = remaining % 60;
    format!("1970-01-{:02}T{:02}:{:02}:{:02}Z", days + 1, h, m, s)
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
            app_hash: None,
            rt_deps_hash: None,
        };
        let json = build_meta_json(
            "myapp",
            "python",
            0,
            &["python3".into(), "app.py".into()],
            &[("PORT".into(), "8000".into())],
            &[],
            &opts,
        );
        let parsed: serde_json::Value = serde_json::from_slice(&json).unwrap();
        assert_eq!(parsed["name"], "myapp");
        assert_eq!(parsed["runtime"], "python");
        assert_eq!(parsed["version"], "1.0");
    }
}
