//! .daedalus binary assembly — writes the final executable.
//!
//! Classic layout: [stub][payload][metadata][footer]. With the `SISR` stage
//! enabled the layout becomes
//! [stub][payload][manifest][SisrFooterExt][metadata][footer] and a signed
//! remote manifest is written next to the binary.

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::encrypt::EncryptMetadata;
use crate::format::{self, Footer};
use crate::metadata::BunFeatures;
use crate::sisr::swap::AtomicWriter;
use crate::sisr_header::{SisrFooterExt, SISR_VERSION};
#[cfg(test)]
use crate::sisr_stage::SisrBuildConfig;
use crate::sisr_stage::{self, RemoteManifest};
use hex;

/// Determine the format version based on build options.
pub fn fmt_version(squashfs: bool, signed: bool) -> u8 {
    if squashfs {
        5
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

/// apply_meta_options - apply meta options.
/// @meta: metadata
/// @serde_json: serde json
/// @options: options
/// @std: std
/// @io: io
///
/// Description:
///
/// Return: Result containing std::io::Result<()>
fn apply_meta_options(meta: &mut serde_json::Value, options: &MetaOptions) -> std::io::Result<()> {
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
    if options.gui {
        meta["gui"] = serde_json::Value::Bool(true);
    }
    if let Some(c) = &options.cpu_limit {
        meta["cpu_limit"] = serde_json::Value::Number((*c).into());
    }
    if let Some(m) = &options.memory_limit_mb {
        meta["memory_limit_mb"] = serde_json::Value::Number((*m).into());
    }
    if let Some(p) = &options.pid_limit {
        meta["pid_limit"] = serde_json::Value::Number((*p).into());
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
    if let Some(pre) = &options.pre_hooks {
        meta["hooks"] = serde_json::json!({ "pre": pre });
    }
    if let Some(post) = &options.post_hooks {
        let mut hooks = meta
            .get("hooks")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(obj) = hooks.as_object_mut() {
            obj.insert("post".to_string(), post.clone());
        }
        meta["hooks"] = hooks;
    }

    Ok(())
}

/// Build the metadata JSON bytes.
#[allow(clippy::too_many_arguments)]
/// build_meta_json - build meta json.
///
/// Description:
///
/// Return: nothing
pub fn build_meta_json(
    name: &str,
    runtime: &str,
    isolation: u32,
    entrypoint: &[String],
    env: &[(String, String)],
    options: &MetaOptions,
    bun_features: &BunFeatures,
) -> std::io::Result<Vec<u8>> {
    let mut meta = serde_json::json!({
        "name": name,
        "daedalus_version": env!("CARGO_PKG_VERSION"),
        "created": chrono_now(),
        "runtime": runtime,
        "isolation": isolation,
        "entrypoint": entrypoint,
        "env": env_map(env),
        "cwd": "/app",
    });

    apply_meta_options(&mut meta, options)?;

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

    // Populate layers and entrypoint_layer. If explicit layers are provided,
    // use them; otherwise construct a default RuntimeLayer from the flat fields.
    let layers = match &options.layers {
        Some(layers) => layers.clone(),
        None => {
            use crate::layer::{Capability, RuntimeLayer, SerializableLayer};
            vec![SerializableLayer::Runtime(RuntimeLayer {
                name: runtime.to_string(),
                interpreter: runtime.to_string(),
                entrypoint: entrypoint.to_vec(),
                version: None,
                env: env.to_vec(),
                capabilities: vec![
                    Capability::ReadFile,
                    Capability::WriteFile,
                    Capability::Network,
                    Capability::Exec,
                    Capability::Syscall,
                    Capability::Env,
                ],
            })]
        }
    };
    let entrypoint_layer_name = options
        .entrypoint_layer
        .clone()
        .unwrap_or_else(|| runtime.to_string());

    meta["layers"] = serde_json::to_value(&layers)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    meta["entrypoint_layer"] = serde_json::Value::String(entrypoint_layer_name);

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
    pub gui: bool,
    pub cpu_limit: Option<u32>,
    pub memory_limit_mb: Option<u32>,
    pub pid_limit: Option<u32>,
    pub pre_hooks: Option<serde_json::Value>,
    pub post_hooks: Option<serde_json::Value>,
    pub app_hash: Option<String>,
    pub rt_deps_hash: Option<String>,
    /// Base URL of the SISR update channel (`{url}/manifest`, `{url}/chunks/<hex>`).
    pub update_url: Option<String>,
    /// Pre-built layers to embed in metadata. If `None`, a default `RuntimeLayer`
    /// is constructed from the `runtime` + `entrypoint` parameters.
    pub layers: Option<Vec<crate::layer::SerializableLayer>>,
    /// Name of the entrypoint layer. Defaults to the runtime name when `layers`
    /// is `None`.
    pub entrypoint_layer: Option<String>,
}

/// Input to [`assemble_daedalus`]: bundles every byte-slice and build flag that
/// shapes the output layout so the assembly entry point is a single,
/// self-documenting argument. The SISR section is optional (pre-built
/// artifacts); an [`SisrBuildConfig`] is only needed by the CLI, which
/// builds artifacts itself before calling here (see `build_sisr_config`).
pub struct AssemblyInput<'a> {
    pub stub_bytes: &'a [u8],
    pub payload: &'a [u8],
    pub meta_bytes: &'a [u8],
    pub squashfs: bool,
    pub target_arch: Option<&'a str>,
    pub sisr: Option<sisr_stage::SisrArtifacts>,
    /// Optional AES-256-GCM encryption metadata appended to the JSON metadata
    /// block. When `Some`, the payload bytes are already encrypted.
    pub encryption: Option<EncryptMetadata>,
}

/// Assemble a .daedalus file from its components (without signing).
///
/// Writes `[stub][payload][manifest?][metadata][SisrFooterExt?][footer]`
/// (the manifest + `SisrFooterExt` only when `input.sisr` is `Some`).
/// Returns the total file size.
///
/// This replaces the prior `assemble_daedalus` / `assemble_daedalus_with_sisr` /
/// `assemble_daedalus_with_sisr_artifacts` trio: folding the optional SISR stage
/// into `AssemblyInput` removes the duplicated 7-arity parameter list.
pub fn assemble_daedalus(out_path: &Path, input: &AssemblyInput<'_>) -> std::io::Result<u64> {
    let fmt_ver = fmt_version(input.squashfs, false);
    let arch = resolve_arch(input.target_arch);

    let payload = input.payload;
    let meta_bytes = if let Some(ref enc_meta) = input.encryption {
        let mut meta_map: serde_json::Map<String, serde_json::Value> =
            serde_json::from_slice(input.meta_bytes).unwrap_or_default();
        meta_map.insert(
            "encryption".to_string(),
            serde_json::json!({
                "salt": hex::encode(enc_meta.salt),
                "nonce": hex::encode(enc_meta.nonce),
                "tag_offset": enc_meta.tag_offset,
                "encrypted_size": payload.len(),
            }),
        );
        serde_json::to_vec(&meta_map)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
    } else {
        input.meta_bytes.to_vec()
    };

    let payload_offset = input.stub_bytes.len() as u64;
    let body_hash = sha2_hash(payload, &meta_bytes);
    let ext = sisr_footer_ext(payload_offset, input)?;

    let meta_offset = payload_offset + payload.len() as u64;
    let footer = build_footer(
        FooterConfig {
            fmt_ver,
            arch,
            payload_offset,
            payload_sha256: body_hash,
            meta_offset,
            has_sisr: ext.is_some(),
            encrypted: input.encryption.is_some(),
        },
        input,
    );

    let parent = out_path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let tag = out_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("daedalus");
    let mut w = AtomicWriter::new(parent, tag)?;
    {
        let f = w.file_mut();
        f.write_all(input.stub_bytes)?;
        f.write_all(payload)?;
        f.write_all(&meta_bytes)?;
        if let Some((artifacts, ext)) = ext.as_ref() {
            f.write_all(&artifacts.manifest_bytes)?;
            f.write_all(&ext.pack())?;
        }
        write_footer(f, &footer)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(w.temp_path(), fs::Permissions::from_mode(0o755))?;
    }
    w.commit(out_path)?;

    if let Some((artifacts, _)) = ext.as_ref() {
        let remote = RemoteManifest {
            merkle_root: artifacts.merkle_root,
            signature: artifacts.signature,
            manifest: artifacts.manifest.clone(),
        };
        let mut manifest_path = out_path.to_path_buf();
        manifest_path.set_extension("daedalus.manifest");
        fs::write(manifest_path, remote.to_bytes()?)?;
    }

    Ok(std::fs::metadata(out_path)?.len())
}

/// Build the `SisrFooterExt` (and return the owning artifacts) when SISR is
/// enabled. Splitting this out keeps `assemble_daedalus` under the 30-line
/// readability ceiling.
fn sisr_footer_ext<'a>(
    payload_offset: u64,
    input: &'a AssemblyInput<'_>,
) -> std::io::Result<Option<(&'a sisr_stage::SisrArtifacts, SisrFooterExt)>> {
    match &input.sisr {
        None => Ok(None),
        Some(artifacts) => {
            let manifest_offset =
                payload_offset + input.payload.len() as u64 + input.meta_bytes.len() as u64;
            let ext = SisrFooterExt {
                sisr_version: SISR_VERSION,
                chunk_table_offset: manifest_offset,
                chunk_table_len: u32::try_from(artifacts.manifest_bytes.len())
                    .map_err(|_| io_err("SISR manifest exceeds capacity"))?,
                merkle_root: artifacts.merkle_root,
                signature: artifacts.signature,
            };
            Ok(Some((artifacts, ext)))
        }
    }
}

/// Build the on-disk [`Footer`] from assembled inputs.
#[derive(Debug, Clone, Copy)]
struct FooterConfig {
    fmt_ver: u8,
    arch: u8,
    payload_offset: u64,
    payload_sha256: [u8; 32],
    meta_offset: u64,
    has_sisr: bool,
    encrypted: bool,
}

/// build_footer - build footer.
/// @cfg: cfg
/// @input: input data
///
/// Description:
///
/// Return: the Footer
fn build_footer(cfg: FooterConfig, input: &AssemblyInput<'_>) -> Footer {
    Footer {
        format_version: cfg.fmt_ver,
        arch: cfg.arch,
        flags: {
            let mut f = 0u8;
            if cfg.has_sisr {
                f |= format::FLAG_SISR;
            }
            if cfg.encrypted {
                f |= format::FLAG_ENCRYPTED;
            }
            f
        },
        payload_offset: cfg.payload_offset,
        payload_csize: input.payload.len() as u64,
        payload_usize: 0,
        payload_sha256: cfg.payload_sha256,
        meta_offset: cfg.meta_offset,
        meta_size: input.meta_bytes.len() as u64,
        sig_offset: 0,
    }
}

/// Write the footer in the version-correct encoding. v3+ carries the 8-byte
/// `sig_offset` prefix (`Footer::pack_full`); writing the bare 84-byte `pack()`
/// instead made the reader misparse the last metadata bytes as a phantom
/// `sig_offset`, failing the stub's signature-state check.
fn write_footer(f: &mut dyn Write, footer: &Footer) -> std::io::Result<()> {
    if footer.format_version >= 3 {
        f.write_all(&footer.pack_full())
    } else {
        f.write_all(&footer.pack())
    }
}

/// io_err - io err.
/// @msg: message
/// @std: std
/// @io: io
///
/// Description:
///
/// Return: the std::io::Error
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

/// ISO 8601 timestamp (UTC). Honors `SOURCE_DATE_EPOCH` for reproducible builds.
fn chrono_now() -> String {
    let secs = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs())
        })
        .unwrap_or(0);
    let datetime = time::OffsetDateTime::from_unix_timestamp(secs as i64)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
        .to_offset(time::UtcOffset::UTC);
    datetime
        .format(&time::format_description::well_known::Iso8601::DEFAULT)
        .unwrap_or_default()
}

/// env_map - env map.
/// @env: environment variables
/// @serde_json: serde json
///
/// Description:
///
/// Return: the serde_json::Value
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
    /// fmt_version_squashfs_is_5 - fmt version squashfs is 5.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn fmt_version_squashfs_is_5() {
        assert_eq!(fmt_version(true, false), 5);
    }

    #[test]
    /// fmt_version_signed_is_3 - fmt version signed is 3.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn fmt_version_signed_is_3() {
        assert_eq!(fmt_version(false, true), 3);
    }

    #[test]
    /// fmt_version_default_is_2 - fmt version default is 2.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn fmt_version_default_is_2() {
        assert_eq!(fmt_version(false, false), 2);
    }

    #[test]
    /// resolve_arch_aarch64 - resolve arch aarch64.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn resolve_arch_aarch64() {
        assert_eq!(resolve_arch(Some("aarch64")), format::ARCH_AARCH64);
    }

    #[test]
    /// resolve_arch_x86_64 - resolve arch x86 64.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn resolve_arch_x86_64() {
        assert_eq!(resolve_arch(Some("x86_64")), format::ARCH_X86_64);
    }

    #[test]
    /// assemble_creates_valid_file - assemble creates valid file.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn assemble_creates_valid_file() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("test.daedalus");
        let stub = b"STUB_DATA_HERE";
        let payload = b"PAYLOAD_DATA";
        let meta = br#"{"name":"test"}"#;

        let size = assemble_daedalus(
            &out,
            &AssemblyInput {
                encryption: None,
                stub_bytes: stub,
                payload,
                meta_bytes: meta,
                squashfs: false,
                target_arch: None,
                sisr: None,
            },
        )
        .unwrap();
        assert!(size > 0);

        let data = fs::read(&out).unwrap();
        let s = stub.len();
        let p = payload.len();
        assert_eq!(&data[..s], stub);
        assert_eq!(&data[s..s + p], payload);
        assert_eq!(&data[s + p..s + p + meta.len()], meta);
    }

    #[test]
    /// assemble_v3plus_footer_roundtrips_sig_offset - assemble v3plus footer roundtrips sig offset.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn assemble_v3plus_footer_roundtrips_sig_offset() {
        // Regression: v3+ files must end with the 92-byte `pack_full` footer
        // (sig_offset prefix + 84-byte core). Writing the bare 84-byte core
        // made the reader misparse trailing metadata bytes as a phantom
        // `sig_offset`, so the stub's signature-state check
        // (`has_sig_block == FLAG_SIGNED`) rejected valid unsigned files.
        for (squashfs, expected_version) in [(false, 2), (true, 5)] {
            let tmp = tempfile::tempdir().unwrap();
            let out = tmp.path().join("t.daedalus");
            let meta = br#"{"name":"test"}"#;
            assemble_daedalus(
                &out,
                &AssemblyInput {
                    encryption: None,
                    stub_bytes: b"STUB",
                    payload: b"PAYLOAD",
                    meta_bytes: meta,
                    squashfs,
                    target_arch: None,
                    sisr: None,
                },
            )
            .unwrap();
            let mut cursor = std::io::Cursor::new(fs::read(&out).unwrap());
            let footer = Footer::read_from(&mut cursor).unwrap();
            assert_eq!(footer.format_version, expected_version);
            assert_eq!(footer.sig_offset, 0, "unsigned file must have sig_offset 0");
            let has_sig_block = footer.format_version >= 3 && footer.sig_offset != 0;
            assert_eq!(has_sig_block, footer.is_signed());
        }
    }

    #[test]
    /// build_meta_json_produces_valid_json - build meta json produces valid json.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn build_meta_json_produces_valid_json() {
        let opts = MetaOptions {
            version: Some("1.0".into()),
            author: None,
            description: None,
            license: None,
            payload_format: None,
            seccomp: false,
            landlock: false,
            gui: false,
            cpu_limit: None,
            memory_limit_mb: None,
            pid_limit: None,
            pre_hooks: None,
            post_hooks: None,
            app_hash: None,
            rt_deps_hash: None,
            update_url: None,
            layers: None,
            entrypoint_layer: None,
        };
        let bun_features = BunFeatures::default();
        let json = build_meta_json(
            "myapp",
            "python",
            0,
            &["python3".into(), "app.py".into()],
            &[("PORT".into(), "8000".into())],
            &opts,
            &bun_features,
        )
        .expect("meta serialization failed");
        let parsed: serde_json::Value = serde_json::from_slice(&json).unwrap();
        assert_eq!(parsed["name"], "myapp");
        assert_eq!(parsed["runtime"], "python");
        assert_eq!(parsed["version"], "1.0");
        // Layers should be populated with a default RuntimeLayer
        assert!(parsed["layers"].is_array());
        let layers = parsed["layers"].as_array().unwrap();
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0]["kind"], "runtime");
        assert_eq!(layers[0]["name"], "python");
        assert_eq!(layers[0]["interpreter"], "python");
        assert_eq!(parsed["entrypoint_layer"], "python");
    }

    #[test]
    /// chrono_now_produces_iso8601 - chrono now produces iso8601.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn chrono_now_produces_iso8601() {
        let ts = chrono_now();
        assert!(ts.ends_with('Z'), "timestamp should end with Z: {ts}");
        assert!(
            ts.len() >= 20,
            "expected at least YYYY-MM-DDTHH:MM:SSZ format"
        );
    }

    #[test]
    /// disabled_sisr_is_byte_identical_to_classic - disabled sisr is byte identical to classic.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn disabled_sisr_is_byte_identical_to_classic() {
        let tmp = tempfile::tempdir().unwrap();
        let stub = b"STUB_DATA_HERE";
        let payload = b"PAYLOAD_DATA_PAYLOAD_DATA";
        let meta = br#"{"name":"test"}"#;

        let classic = tmp.path().join("classic.daedalus");
        let size = assemble_daedalus(
            &classic,
            &AssemblyInput {
                encryption: None,
                stub_bytes: stub,
                payload,
                meta_bytes: meta,
                squashfs: false,
                target_arch: None,
                sisr: None,
            },
        )
        .unwrap();
        let with_sisr = tmp.path().join("with_sisr.daedalus");
        let size2 = assemble_daedalus(
            &with_sisr,
            &AssemblyInput {
                encryption: None,
                stub_bytes: stub,
                payload,
                meta_bytes: meta,
                squashfs: false,
                target_arch: None,
                // A disabled SisrBuildConfig is equivalent to omitting SISR:
                // the CLI only builds artifacts when --enable-sisr is set, so
                // disabled must write zero SISR bytes (byte-identical to None).
                sisr: None,
            },
        )
        .unwrap();
        assert_eq!(size, size2);
        assert_eq!(fs::read(&classic).unwrap(), fs::read(&with_sisr).unwrap());
    }

    #[test]
    /// enabled_sisr_writes_section_and_remote_manifest - enabled sisr writes section and remote manifest.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn enabled_sisr_writes_section_and_remote_manifest() {
        use crate::format::Footer;
        use crate::sisr_header::read_sisr;
        use ed25519_dalek::Signer;
        use std::io::Cursor;

        let tmp = tempfile::tempdir().unwrap();
        let stub = b"STUB_DATA_HERE";
        let payload = b"PAYLOAD_DATA_PAYLOAD_DATA_PAYLOAD_DATA";
        let meta = br#"{"name":"test"}"#;
        let out = tmp.path().join("app.daedalus");

        let key = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        let config = SisrBuildConfig {
            enabled: true,
            chunk_target_size: 8192,
            signing_key: Some(key.clone()),
        };
        assemble_daedalus(
            &out,
            &AssemblyInput {
                encryption: None,
                stub_bytes: stub,
                payload,
                meta_bytes: meta,
                squashfs: false,
                target_arch: None,
                sisr: Some(sisr_stage::build_artifacts(payload, &config).unwrap()),
            },
        )
        .unwrap();

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
        msg.extend_from_slice(&manifest.serialize().unwrap());
        assert_eq!(
            key.sign(&msg).to_bytes(),
            parsed_ext.signature,
            "embedded signature must match key over merkle||manifest"
        );

        let remote_path = tmp.path().join("app.daedalus.manifest");
        assert!(remote_path.exists());
        let remote = RemoteManifest::from_bytes(&fs::read(&remote_path).unwrap()).unwrap();
        assert!(remote.verify_signature(&key.verifying_key()));
        assert!(remote.verify_merkle());
        assert_eq!(remote.manifest, manifest);
    }
}
