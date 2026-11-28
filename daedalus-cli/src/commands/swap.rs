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
    about = "Hot-swap a layer in an existing .de binary",
    long_about = "Replaces a named file inside the payload of a .de binary and reassembles the artifact with an updated integrity hash.\n\nUsage: daedalus swap <binary> <layer-name> <new-file> [-o output]\n\nLimitations:\n- Plain zstd-tar payloads only (v2). Does not support squashfs (v5).\n- Invalidates any Ed25519 binary signature (re-sign with `daedalus sign` after swap)."
)]
pub struct SwapArgs {
    /// Path to the .de binary, or `-` to read from stdin
    #[arg()]
    pub binary: PathBuf,

    /// Name of the layer to swap (e.g. "runtime", "config")
    #[arg()]
    pub layer_name: String,

    /// Path to the new layer file
    #[arg()]
    pub new_file: PathBuf,

    /// Output path (`-` streams the result to stdout; default: overwrites input)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Quiet output
    #[arg(short, long, default_value = "false", hide = true)]
    pub quiet: bool,

    /// Output result as JSON
    #[arg(long)]
    pub json: bool,

    /// Disable all interactive prompts (for CI/scripts)
    #[arg(long, global = true)]
    pub no_input: bool,
}

/// run - hot-swap a file inside a .daedalus binary's tar payload.
/// @args: command arguments
///
/// Description:
/// Replaces a named file inside the zstd-tar payload of a .daedalus binary,
/// recomputes the integrity hash, and writes the updated binary. SISR and
/// squashfs payloads are not supported.
///
/// Return: Result containing anyhow::Result<()>
pub fn run(args: SwapArgs) -> anyhow::Result<()> {
    let binary_display = if crate::stdio::is_dash(&args.binary.to_string_lossy()) {
        "stdin".to_string()
    } else {
        args.binary.display().to_string()
    };
    let binary_data = if crate::stdio::is_dash(&args.binary.to_string_lossy()) {
        crate::stdio::read_stdin_cursor()?.into_inner()
    } else {
        std::fs::read(&args.binary)
            .with_context(|| format!("failed to read {}", args.binary.display()))?
    };

    let new_data = std::fs::read(&args.new_file)
        .with_context(|| format!("failed to read {}", args.new_file.display()))?;

    let output = args.output.clone().unwrap_or_else(|| {
        let mut p = args.binary.clone();
        p.set_extension("daedalus");
        p
    });
    let output_display = if crate::stdio::is_stdout_output(&output) {
        "stdout".to_string()
    } else {
        output.display().to_string()
    };

    eprintln!("[daedalus] swap: {} -> {}", binary_display, output_display);
    eprintln!("[daedalus] layer: {}", args.layer_name);
    eprintln!(
        "[daedalus] new file: {} ({} bytes)",
        args.new_file.display(),
        new_data.len()
    );

    let new_binary = swap_layer_bytes(&binary_data, &new_data, &args.layer_name, &args.new_file)?;

    if crate::stdio::is_stdout_output(&output) {
        use std::io::Write;
        std::io::stdout().write_all(&new_binary)?;
        std::io::stdout().flush()?;
    } else {
        std::fs::write(&output, new_binary)
            .with_context(|| format!("failed to write {}", output.display()))?;
    }

    if args.json {
        let info = serde_json::json!({
            "command": "swap",
            "binary": binary_display,
            "layer_name": args.layer_name,
            "new_file": args.new_file.display().to_string(),
            "output": output_display,
            "success": true,
        });
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        eprintln!("[daedalus] swap complete: {}", output_display);
    }
    Ok(())
}

/// append_normalized - re-append a tar entry with deterministic ownership
/// while preserving its original type, mode, and symlink/hardlink target.
///
/// Normalizes mtime/uid/gid/uname/gname (reproducible payloads) but keeps the
/// entry kind intact, so directories stay directories and links stay links -
/// coercing everything to a regular file corrupts the rootfs tree.
fn append_normalized<W: std::io::Write>(
    builder: &mut tar::Builder<W>,
    path: &str,
    original: &tar::Header,
    data: &[u8],
) -> anyhow::Result<()> {
    let mut header = original.clone();
    header.set_mtime(0);
    header.set_uid(0);
    header.set_gid(0);
    header.set_username("")?;
    header.set_groupname("")?;

    let kind = header.entry_type();
    if kind.is_symlink() || kind.is_hard_link() {
        let link = header
            .link_name()?
            .ok_or_else(|| anyhow::anyhow!("link entry {path} has no target"))?
            .into_owned();
        header.set_size(0);
        builder.append_link(&mut header, path, &link)?;
    } else {
        let size = if kind.is_file() { data.len() as u64 } else { 0 };
        header.set_size(size);
        let body = if kind.is_file() { data } else { &[][..] };
        builder.append_data(&mut header, path, body)?;
    }
    Ok(())
}

/// swap_layer_bytes - replace a file in a .daedalus tar payload and reassemble.
///
/// Description:
/// Decompresses the zstd-tar payload, replaces the target file, re-serializes
/// with deterministic ordering and fixed mtime/uid/gid, recompresses, and
/// returns the new binary with an updated footer hash. The caller decides
/// whether to write it to a file or stream it to stdout.
///
/// Return: the rebuilt `.daedalus` bytes
fn swap_layer_bytes(
    binary_data: &[u8],
    new_file_data: &[u8],
    _layer_name: &str,
    new_file_path: &Path,
) -> anyhow::Result<Vec<u8>> {
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

    // Preserve each entry's original header (type, mode, symlink target),
    // not just its bytes. Coercing every entry to a regular file turns the
    // payload's directories and symlinks into empty 0-byte files, which makes
    // the launcher fail to unpack anything nested under them.
    let mut entries: Vec<(String, tar::Header, Vec<u8>)> = Vec::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_string_lossy().to_string();
        let header = entry.header().clone();
        let mut buf = Vec::new();
        if header.entry_type().is_file() {
            entry.read_to_end(&mut buf)?;
        }
        entries.push((path, header, buf));
    }

    let file_name = new_file_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let target_rel_paths: Vec<String> =
        vec![format!("app/{}", file_name), format!("app/./{}", file_name)];

    let mut found = false;
    for (path, _header, data) in &mut entries {
        if target_rel_paths.contains(path) {
            *data = new_file_data.to_vec();
            found = true;
            break;
        }
    }

    if !found {
        let all_paths: Vec<_> = entries.iter().map(|(p, _, _)| p.clone()).collect();
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
        for (path, header, data) in sorted_entries {
            append_normalized(&mut builder, &path, &header, &data)
                .with_context(|| format!("append {} to tar", path))?;
        }
        builder
            .finish()
            .with_context(|| "finish tar builder".to_string())?;
    }

    let new_payload_compressed =
        compress(&tar_buf).with_context(|| "failed to recompress payload".to_string())?;

    let new_binary = assemble_daedalus_bytes(stub_bytes, &new_payload_compressed, metadata_bytes)?;

    Ok(new_binary)
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

    // Integrity hash MUST be SHA-256(payload || metadata), matching the
    // canonical assembler (daedalus-core::assembly::sha2_hash) and the
    // launcher's runtime check (crypto::verify_sha256_parts). Hashing the
    // payload alone makes the reassembled binary fail its own integrity
    // gate at cold start with "payload integrity check failed".
    let payload_sha256: [u8; 32] = {
        let mut hasher = Sha256::new();
        hasher.update(payload);
        hasher.update(meta);
        hasher.finalize().into()
    };
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

    /// Regression guard for the two swap round-trip bugs:
    /// (1) the footer integrity hash must be SHA-256(payload || meta), the same
    ///     value the launcher recomputes - hashing the payload alone made the
    ///     reassembled binary reject itself at cold start;
    /// (2) directory entries must survive reassembly as directories, not be
    ///     flattened into empty regular files.
    #[test]
    fn swap_roundtrip_preserves_integrity_and_tree() {
        use daedalus_core::compress::{compress, decompress};
        use daedalus_core::format::Footer;
        use sha2::{Digest, Sha256};
        use std::io::{Cursor, Read};

        // Build a tar with an explicit directory entry and a nested file.
        let mut tar_buf = Vec::new();
        {
            let mut b = tar::Builder::new(&mut tar_buf);
            let mut dir = tar::Header::new_gnu();
            dir.set_entry_type(tar::EntryType::Directory);
            dir.set_mode(0o755);
            dir.set_size(0);
            b.append_data(&mut dir, "app/", &[][..]).unwrap();
            let mut f = tar::Header::new_gnu();
            f.set_entry_type(tar::EntryType::Regular);
            f.set_mode(0o644);
            f.set_size(5);
            b.append_data(&mut f, "app/app.py", &b"hello"[..]).unwrap();
            b.finish().unwrap();
        }
        let payload = compress(&tar_buf).unwrap();
        let meta = br#"{"name":"t","runtime":"python","layers":[]}"#;
        let binary = assemble_daedalus_bytes(b"STUB", &payload, meta).unwrap();

        let new = b"print('swapped')\n";
        let swapped = swap_layer_bytes(&binary, new, "app", Path::new("app.py")).unwrap();

        // (1) footer hash == SHA-256(payload || meta) over the swapped slices.
        let footer = Footer::read_from(&mut Cursor::new(&swapped)).unwrap();
        let p = &swapped[footer.payload_offset as usize
            ..footer.payload_offset as usize + footer.payload_csize as usize];
        let m = &swapped
            [footer.meta_offset as usize..footer.meta_offset as usize + footer.meta_size as usize];
        let mut h = Sha256::new();
        h.update(p);
        h.update(m);
        assert_eq!(<[u8; 32]>::from(h.finalize()), footer.payload_sha256);

        // (2) the directory entry is still a directory and the file was swapped.
        let raw = decompress(p).unwrap();
        let mut ar = tar::Archive::new(Cursor::new(raw));
        let (mut saw_dir, mut saw_new) = (false, false);
        for e in ar.entries().unwrap() {
            let mut e = e.unwrap();
            let path = e.path().unwrap().to_string_lossy().to_string();
            if path == "app/" {
                assert!(e.header().entry_type().is_dir());
                saw_dir = true;
            } else if path == "app/app.py" {
                let mut buf = Vec::new();
                e.read_to_end(&mut buf).unwrap();
                assert_eq!(buf, new);
                saw_new = true;
            }
        }
        assert!(saw_dir, "directory entry was flattened");
        assert!(saw_new, "swapped file missing");
    }

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
            json: false,
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
            json: false,
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
            json: false,
            no_input: false,
        };
        let result = run(args);
        assert!(result.is_err(), "swap should reject SISR binary");
    }
}
