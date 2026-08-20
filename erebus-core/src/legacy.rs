//! Legacy (v1) → SISR (v2) migration.
//!
//! [`upgrade_binary`] converts a classic `.erebus` (no `FLAG_SISR`) into a
//! SISR-enabled binary without touching the payload. The stub, payload, and
//! metadata segments are copied through byte-for-byte; a delta manifest and a
//! [`SisrFooterExt`] are inserted between the metadata and the footer.
//!
//! ```text
//! before: [stub][payload][metadata][footer]
//! after:  [stub][payload][metadata][manifest][SisrFooterExt][footer]
//! ```
//!
//! Because every pre-existing offset (`payload_offset`, `meta_offset`) is
//! unchanged, the footer integrity hash `SHA-256(payload ‖ meta)` — and any
//! checksum the payload itself embeds, e.g. `SquashFS` — is preserved by
//! construction. A legacy runtime that reads backwards from EOF keeps decoding
//! the upgraded file, while the v2 runtime additionally sees `FLAG_SISR` and
//! gains delta auto-update support.

use std::fs;
use std::io;
use std::path::Path;

use crate::format::{self, Footer};
use crate::sisr_header::{SisrFooterExt, SISR_VERSION};
use crate::sisr_stage::{self, RemoteManifest, SisrBuildConfig};

/// Result of a successful [`upgrade_binary`] run.
#[derive(Debug)]
pub struct UpgradeReport {
    pub input_size: u64,
    pub output_size: u64,
    pub chunk_count: usize,
    pub manifest_offset: u64,
    pub signed: bool,
}

/// Converts a classic (SISR-less) `.erebus` into a SISR-enabled one.
///
/// The payload is chunked exactly as it is stored on disk — never
/// decompressed and recompressed — so the stored bytes (and therefore any
/// payload-internal checksums) are unchanged. Also writes `<output>.erebus.manifest`
/// next to the binary, matching [`crate::assembly::assemble_erebus`].
pub fn upgrade_binary(
    input: &Path,
    output: &Path,
    config: &SisrBuildConfig,
) -> io::Result<UpgradeReport> {
    if !config.enabled {
        return Err(err("upgrade requires the SISR stage to be enabled"));
    }

    let data = fs::read(input)?;
    let footer = Footer::read_from(&mut io::Cursor::new(&data))?;

    if footer.has_sisr() {
        return Err(err("input is already SISR-enabled"));
    }
    if footer.format_version >= 3 || footer.flags & format::FLAG_SIGNED != 0 {
        return Err(err(
            "signed binaries are not upgradeable — rebuild with `erebus build --enable-sisr` instead",
        ));
    }

    let payload_start = usize::try_from(footer.payload_offset)
        .map_err(|_| err("payload offset overflows this platform"))?;
    let meta_start = usize::try_from(footer.meta_offset)
        .map_err(|_| err("metadata offset overflows this platform"))?;
    let payload_end = usize::try_from(footer.payload_offset + footer.payload_csize)
        .map_err(|_| err("payload range overflows this platform"))?;
    let meta_end = usize::try_from(footer.meta_offset + footer.meta_size)
        .map_err(|_| err("metadata range overflows this platform"))?;
    let footer_len = usize::try_from(footer.footer_size())
        .map_err(|_| err("footer size overflows this platform"))?;

    let tail = data
        .len()
        .checked_sub(footer_len)
        .ok_or_else(|| err("file too small"))?;
    if payload_end > meta_start || meta_end > tail {
        return Err(err(
            "malformed .erebus: overlapping or out-of-bounds segments",
        ));
    }

    let payload = &data[payload_start..payload_end];
    let artifacts = sisr_stage::build_artifacts(payload, config)?;
    let chunk_count = artifacts.manifest.chunks.len();

    let manifest_offset = footer.meta_offset + footer.meta_size;
    let manifest_len = u32::try_from(artifacts.manifest_bytes.len())
        .map_err(|_| err("SISR manifest exceeds capacity"))?;

    let mut out_footer = footer;
    out_footer.flags |= format::FLAG_SISR;

    let ext = SisrFooterExt {
        sisr_version: SISR_VERSION,
        chunk_table_offset: manifest_offset,
        chunk_table_len: manifest_len,
        merkle_root: artifacts.merkle_root,
        signature: artifacts.signature,
    };

    let mut out = Vec::with_capacity(
        data.len() + artifacts.manifest_bytes.len() + format::SISR_FOOTER_EXT_SIZE,
    );
    out.extend_from_slice(&data[..meta_end]);
    out.extend_from_slice(&artifacts.manifest_bytes);
    out.extend_from_slice(&ext.pack());
    out.extend_from_slice(&out_footer.pack());

    fs::write(output, &out)?;
    set_executable(output)?;

    let remote = RemoteManifest {
        merkle_root: artifacts.merkle_root,
        signature: artifacts.signature,
        manifest: artifacts.manifest,
    };
    let mut manifest_path = output.to_path_buf();
    manifest_path.set_extension("erebus.manifest");
    fs::write(manifest_path, remote.to_bytes()?)?;

    Ok(UpgradeReport {
        input_size: data.len() as u64,
        output_size: out.len() as u64,
        chunk_count,
        manifest_offset,
        signed: config.signing_key.is_some(),
    })
}

fn set_executable(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn err(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::{assemble_erebus, AssemblyInput};
    use crate::sisr_header::read_sisr;
    use ed25519_dalek::SigningKey;
    use std::io::Cursor;

    fn fixture_v1(dir: &Path) -> std::path::PathBuf {
        let out = dir.join("legacy.erebus");
        assemble_erebus(
            &out,
            &AssemblyInput {
                stub_bytes: b"STUB_DATA",
                payload: b"PAYLOAD_PAYLOAD_PAYLOAD",
                meta_bytes: br#"{"name":"legacy"}"#,
                encrypt: false,
                squashfs: false,
                target_arch: None,
                sisr: None,
            },
        )
        .unwrap();
        out
    }

    fn config() -> SisrBuildConfig {
        SisrBuildConfig {
            enabled: true,
            chunk_target_size: 8 << 10,
            signing_key: Some(SigningKey::from_bytes(&[7u8; 32])),
        }
    }

    #[test]
    fn upgrade_preserves_segments_and_sets_sisr() {
        let tmp = tempfile::tempdir().unwrap();
        let input = fixture_v1(tmp.path());
        let before = fs::read(&input).unwrap();
        let in_footer = Footer::read_from(&mut Cursor::new(&before)).unwrap();

        let out = tmp.path().join("upgraded.erebus");
        let report = upgrade_binary(&input, &out, &config()).unwrap();

        let after = fs::read(&out).unwrap();
        let out_footer = Footer::read_from(&mut Cursor::new(&after)).unwrap();

        // Stub, payload, and metadata segments are byte-identical.
        let payload_end = (in_footer.payload_offset + in_footer.payload_csize) as usize;
        assert_eq!(&after[..payload_end], &before[..payload_end]);
        let meta_end = (in_footer.meta_offset + in_footer.meta_size) as usize;
        assert_eq!(&after[..meta_end], &before[..meta_end]);

        // The footer gained FLAG_SISR but kept its integrity hash and offsets.
        assert_ne!(out_footer.flags & format::FLAG_SISR, 0);
        assert_eq!(out_footer.payload_sha256, in_footer.payload_sha256);
        assert_eq!(out_footer.payload_offset, in_footer.payload_offset);
        assert_eq!(out_footer.meta_offset, in_footer.meta_offset);

        // The manifest round-trips and commits to the stored payload.
        let (ext, manifest) = read_sisr(&mut Cursor::new(&after))
            .unwrap()
            .expect("upgraded file has SISR");
        assert_eq!(manifest.payload_len, in_footer.payload_csize);
        assert_eq!(
            ext.merkle_root,
            crate::sisr_stage::merkle_root_of(&manifest)
        );
        assert_eq!(ext.chunk_table_offset, report.manifest_offset);
        assert_eq!(report.chunk_count, manifest.chunks.len());
        assert!(report.signed);

        // The remote manifest was written next to the binary.
        let remote_path = tmp.path().join("upgraded.erebus.manifest");
        let remote = RemoteManifest::from_bytes(&fs::read(remote_path).unwrap()).unwrap();
        assert!(remote.verify_merkle());
        assert!(report.output_size > report.input_size);
    }

    #[test]
    fn upgrade_rejects_already_sisr() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("sisr.erebus");
        let artifacts =
            crate::sisr_stage::build_artifacts(b"PAYLOAD_PAYLOAD_PAYLOAD", &config()).unwrap();
        assemble_erebus(
            &input,
            &AssemblyInput {
                stub_bytes: b"STUB_DATA",
                payload: b"PAYLOAD_PAYLOAD_PAYLOAD",
                meta_bytes: br#"{"name":"legacy"}"#,
                encrypt: false,
                squashfs: false,
                target_arch: None,
                sisr: Some(artifacts),
            },
        )
        .unwrap();
        let err = upgrade_binary(&input, &tmp.path().join("out.erebus"), &config()).unwrap_err();
        assert!(err.to_string().contains("already SISR"));
    }

    #[test]
    fn upgrade_rejects_signed_input() {
        let tmp = tempfile::tempdir().unwrap();
        let input = fixture_v1(tmp.path());
        // Mark the file as v3 (signed footer) by patching the version byte.
        let mut data = fs::read(&input).unwrap();
        let last = data.len() - 84;
        data[last + 5] = 3;
        let signed = tmp.path().join("signed.erebus");
        fs::write(&signed, &data).unwrap();
        let err = upgrade_binary(&signed, &tmp.path().join("out.erebus"), &config()).unwrap_err();
        assert!(err.to_string().contains("not upgradeable"));
    }

    #[test]
    fn upgrade_rejects_non_erebus() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("junk.bin");
        fs::write(&input, b"this is not an erebus file at all").unwrap();
        assert!(upgrade_binary(&input, &tmp.path().join("out.erebus"), &config()).is_err());
    }
}
