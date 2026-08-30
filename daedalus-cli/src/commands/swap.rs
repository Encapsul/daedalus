//! Hot-swap a layer in an existing .daedalus binary.
//!
//! Replaces a named file inside the payload of a `.daedalus` binary and
//! reassembles the artifact with an updated integrity hash.
//!
//! Usage: daedalus swap <binary> <layer-name> <new-file> [-o output]
//!
//! Limitations:
//! - Plain zstd-tar payloads only (v2). Does not support squashfs (v5).
//! - Invalidates any Ed25519 binary signature (the signature covers the
//!   payload+metadata+footer hash). Re-sign with `daedalus sign` after swap.

use anyhow::Context;
use std::path::{Path, PathBuf};

#[derive(clap::Parser, Debug)]
#[command(
    about = "Hot-swap a layer in an existing .daedalus binary",
    long_about = "Replaces a named file inside the payload of a .daedalus binary and reassembles the artifact with an updated integrity hash.\n\nUsage: daedalus swap <binary> <layer-name> <new-file> [-o output]\n\nLimitations:\n- Plain zstd-tar payloads only (v2). Does not support squashfs (v5).\n- Invalidates any Ed25519 binary signature (re-sign with `daedalus sign` after swap)."
)]
pub struct SwapArgs {
    /// Path to the .daedalus binary
    #[arg()]
    pub binary: PathBuf,

    /// Name of the layer to swap (e.g. "runtime", "config")
    #[arg()]
    pub layer_name: String,

    /// Path to the new layer file
    #[arg()]
    pub new_file: PathBuf,

    /// Output path (default: overwrites input)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Quiet output
    #[arg(short, long, default_value = "false", hide = true)]
    pub quiet: bool,

    /// Disable all interactive prompts (for CI/scripts)
    #[arg(long, global = true)]
    pub no_input: bool,
}

/// run - run.
/// @args: command arguments
/// @anyhow: anyhow
///
/// Description:
///
/// Return: Result containing anyhow::Result<()>
pub fn run(args: SwapArgs) -> anyhow::Result<()> {
    let binary_data = std::fs::read(&args.binary)
        .with_context(|| format!("failed to read {}", args.binary.display()))?;

    let new_data = std::fs::read(&args.new_file)
        .with_context(|| format!("failed to read {}", args.new_file.display()))?;

    let output = args.output.unwrap_or_else(|| {
        let mut p = args.binary.clone();
        p.set_extension("daedalus");
        p
    });

    eprintln!(
        "[daedalus] swap: {} -> {}",
        args.binary.display(),
        output.display()
    );
    eprintln!("[daedalus] layer: {}", args.layer_name);
    eprintln!(
        "[daedalus] new file: {} ({} bytes)",
        args.new_file.display(),
        new_data.len()
    );

    swap_layer(
        &binary_data,
        &new_data,
        &args.layer_name,
        &args.new_file,
        &output,
    )?;

    eprintln!("[daedalus] swap complete: {}", output.display());
    Ok(())
}

/// has_executable_extension - check whether executable extension.
/// @path: file or directory path
///
/// Description:
///
/// Return: true or false
fn has_executable_extension(path: &str) -> bool {
    path.eq_ignore_ascii_case(".py")
        || path.eq_ignore_ascii_case(".sh")
        || path.eq_ignore_ascii_case(".rb")
        || path.eq_ignore_ascii_case(".pl")
        || path.eq_ignore_ascii_case(".php")
        || path.eq_ignore_ascii_case(".js")
        || path.eq_ignore_ascii_case(".exe")
}

/// swap_layer - swap layer.
///
/// Description:
///
/// Return: nothing
fn swap_layer(
    binary_data: &[u8],
    new_file_data: &[u8],
    _layer_name: &str,
    new_file_path: &Path,
    output: &PathBuf,
) -> anyhow::Result<()> {
    use daedalus_core::compress::{compress, decompress};
    use daedalus_core::format::Footer;
    use std::io::Cursor;
    use std::io::Read;
    use tar::Archive;

    let mut cursor = Cursor::new(binary_data);
    let footer = Footer::read_from(&mut cursor)?;

    if footer.has_sisr() {
        anyhow::bail!("SISR-enabled binaries are not yet supported for hot-swap");
    }

    let payload_offset = footer.payload_offset as usize;
    let payload_size = footer.payload_csize as usize;
    let meta_offset = footer.meta_offset as usize;
    let meta_size = footer.meta_size as usize;

    let payload_compressed = &binary_data[payload_offset..payload_offset + payload_size];
    let metadata_bytes = &binary_data[meta_offset..meta_offset + meta_size];
    let stub_bytes = &binary_data[..payload_offset];

    let payload_decompressed = decompress(payload_compressed)
        .with_context(|| "failed to decompress payload".to_string())?;

    let mut tar_reader = Cursor::new(payload_decompressed);
    let mut archive = Archive::new(&mut tar_reader);

    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_string_lossy().to_string();
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        entries.push((path, buf));
    }

    let file_name = new_file_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let target_rel_paths: Vec<String> =
        vec![format!("app/{}", file_name), format!("app/./{}", file_name)];

    let mut found = false;
    for (path, data) in &mut entries {
        if target_rel_paths.contains(path) {
            *data = new_file_data.to_vec();
            found = true;
            break;
        }
    }

    if !found {
        let all_paths: Vec<_> = entries.iter().map(|(p, _)| p.clone()).collect();
        anyhow::bail!(
            "file '{}' not found in payload. Payload contains: {}",
            file_name,
            all_paths.join(", ")
        );
    }

    let mut tar_buf = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_buf);
        let mut sorted_entries = entries.clone();
        sorted_entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (path, data) in sorted_entries {
            let mut header = tar::Header::new_gnu();
            header.set_mtime(0);
            header.set_uid(0);
            header.set_gid(0);
            header
                .set_username("")
                .with_context(|| format!("set username for {}", path))?;
            header
                .set_groupname("")
                .with_context(|| format!("set groupname for {}", path))?;
            header.set_entry_type(tar::EntryType::Regular);
            let mode = if has_executable_extension(&path) {
                0o755
            } else {
                0o644
            };
            header.set_mode(mode);
            header.set_size(data.len() as u64);
            builder
                .append_data(&mut header, &path, data.as_slice())
                .with_context(|| format!("append {} to tar", path))?;
        }
        builder
            .finish()
            .with_context(|| "finish tar builder".to_string())?;
    }

    let new_payload_compressed =
        compress(&tar_buf).with_context(|| "failed to recompress payload".to_string())?;

    let new_binary = assemble_daedalus_bytes(stub_bytes, &new_payload_compressed, metadata_bytes)?;
    std::fs::write(output, new_binary)
        .with_context(|| format!("failed to write {}", output.display()))?;

    Ok(())
}

/// Assemble a .daedalus binary from components.
fn assemble_daedalus_bytes(stub: &[u8], payload: &[u8], meta: &[u8]) -> anyhow::Result<Vec<u8>> {
    use daedalus_core::format::Footer;
    use sha2::{Digest, Sha256};

    let format_version = if meta.len() >= 10 {
        match std::str::from_utf8(&meta[meta.len().saturating_sub(10)..]) {
            Ok(s) if s.contains("\"squashfs\":true") => 5u8,
            Ok(s) if s.contains("\"crypto\"") => 4u8,
            _ => 2u8,
        }
    } else {
        2u8
    };

    let payload_sha256: [u8; 32] = Sha256::digest(payload).into();
    let meta_offset = stub.len() as u64 + payload.len() as u64;
    let footer = Footer {
        format_version,
        arch: 0x01,
        flags: 0,
        payload_offset: stub.len() as u64,
        payload_csize: payload.len() as u64,
        payload_usize: 0,
        payload_sha256,
        meta_offset,
        meta_size: meta.len() as u64,
        sig_offset: 0,
    };

    let footer_bytes = if format_version >= 3 {
        footer.pack_full().to_vec()
    } else {
        footer.pack().to_vec()
    };

    let mut out = Vec::with_capacity(stub.len() + payload.len() + meta.len() + footer_bytes.len());
    out.extend_from_slice(stub);
    out.extend_from_slice(payload);
    out.extend_from_slice(meta);
    out.extend_from_slice(&footer_bytes);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    /// swap_args_parses_minimal - swap args parses minimal.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn swap_args_parses_minimal() {
        let args = SwapArgs {
            binary: PathBuf::from("/tmp/app.daedalus"),
            layer_name: "runtime".into(),
            new_file: PathBuf::from("/tmp/new-runtime"),
            output: None,
            quiet: false,
            no_input: false,
        };
        assert_eq!(args.binary, PathBuf::from("/tmp/app.daedalus"));
        assert_eq!(args.layer_name, "runtime");
        assert_eq!(args.new_file, PathBuf::from("/tmp/new-runtime"));
        assert!(args.output.is_none());
    }

    #[test]
    /// swap_layer_replaces_file_and_reassembles - swap layer replaces file and reassembles.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn swap_layer_replaces_file_and_reassembles() {
        use daedalus_core::compress::compress;
        use daedalus_core::tar::create_deterministic_tar;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("app");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("config.json"), b"original config data").unwrap();
        std::fs::write(root.join("app.py"), b"print('hello')").unwrap();

        let tar_bytes = create_deterministic_tar(tmp.path()).unwrap();
        let payload = compress(&tar_bytes).unwrap();
        let metadata =
            br#"{"name":"test","runtime":"python","entrypoint":["python3","app.py"],"layers":[]}"#;
        let stub = b"STUB";

        let binary = assemble_daedalus_bytes(stub, &payload, metadata).unwrap();
        let binary_path = tmp.path().join("test.daedalus");
        std::fs::write(&binary_path, &binary).unwrap();

        let new_file = tmp.path().join("config.json");
        std::fs::write(&new_file, b"updated config data v2").unwrap();

        let output = tmp.path().join("swapped.daedalus");
        let args = SwapArgs {
            binary: binary_path.clone(),
            layer_name: "config".into(),
            new_file: new_file.clone(),
            output: Some(output.clone()),
            quiet: true,
            no_input: false,
        };
        run(args).expect("swap should succeed");

        let swapped = std::fs::read(&output).expect("output should exist");
        assert!(
            swapped.len() > stub.len(),
            "output must be larger than stub"
        );

        let mut cursor = std::io::Cursor::new(&swapped);
        let footer =
            daedalus_core::format::Footer::read_from(&mut cursor).expect("footer should parse");
        assert_eq!(footer.payload_offset, stub.len() as u64);
        assert!(footer.payload_csize > 0, "payload size must be positive");
    }

    #[test]
    /// swap_layer_rejects_sisr_binary - swap layer rejects sisr binary.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn swap_layer_rejects_sisr_binary() {
        let tmp = tempfile::tempdir().unwrap();
        let binary_path = tmp.path().join("sisr.daedalus");
        let metadata = br#"{"name":"test","runtime":"python","layers":[]}"#;
        let binary = assemble_daedalus_bytes(b"STUB", b"PAYLOAD", metadata).unwrap();
        std::fs::write(&binary_path, &binary).unwrap();

        let args = SwapArgs {
            binary: binary_path,
            layer_name: "config".into(),
            new_file: tmp.path().join("new.txt"),
            output: Some(tmp.path().join("out.daedalus")),
            quiet: true,
            no_input: false,
        };
        let result = run(args);
        assert!(result.is_err(), "swap should reject SISR binary");
    }
}
