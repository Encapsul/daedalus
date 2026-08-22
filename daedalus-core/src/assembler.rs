//! Binary stitching — reassembles a complete executable from a base and
//! payload blocks.
//!
//! The `.daedalus` layout is `[stub][payload][metadata][footer]`. SISR
//! reconstructs the *payload* blocks (e.g. a `SquashFS` image) and stitches
//! them into a base executable. This module provides the trait and a
//! stitcher that glues base + payload blocks into one byte stream, with no
//! external tools (`Command::new(...)` is forbidden here by design).

use std::io::{self, Write};

/// The payload of a `.daedalus` lies between the launcher stub and the metadata.
/// A stitcher that only appends blocks would place them after the footer,
/// breaking the format — so the trait leaves the exact splice point to the
/// implementation.
pub trait BinaryAssembler {
    /// Writes a complete executable to `output`: the base executable with the
    /// reconstructed `payload_blocks` spliced into it.
    fn assemble(
        &self,
        base_exec: &[u8],
        payload_blocks: &[Vec<u8>],
        output: &mut dyn Write,
    ) -> io::Result<()>;
}

/// Stitches base and blocks into the exact `.daedalus` layout.
///
/// The base executable is the whole previous `.daedalus` file. The payload
/// region runs from `payload_offset` to `meta_offset` (per the footer). This
/// stitcher re-emits everything before the payload, then the new payload
/// blocks, then everything after (metadata + footer), preserving the format.
pub struct DaedalusStitcher;

impl BinaryAssembler for DaedalusStitcher {
    fn assemble(
        &self,
        base_exec: &[u8],
        payload_blocks: &[Vec<u8>],
        output: &mut dyn Write,
    ) -> io::Result<()> {
        let (payload_offset, meta_offset) = locate_splice(base_exec)?;
        output.write_all(&base_exec[..payload_offset])?;
        for block in payload_blocks {
            output.write_all(block)?;
        }
        output.write_all(&base_exec[meta_offset..])?;
        Ok(())
    }
}

/// Locates the payload region boundaries using the `.daedalus` footer.
///
/// The footer is the last 92 bytes (v3+) and stores absolute offsets; reading
/// it requires a seekable reader, so we parse the offsets from the raw bytes
/// at the end of the buffer instead.
fn locate_splice(base: &[u8]) -> io::Result<(usize, usize)> {
    const FOOTER_SIZE: usize = 92;
    if base.len() < FOOTER_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "base executable too small to contain a footer",
        ));
    }
    let footer = &base[base.len() - FOOTER_SIZE..];
    let footer_magic = u32::from_le_bytes(
        footer[88..92]
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "footer magic truncated"))?,
    );
    if footer_magic != crate::format::FOOTER_MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "base executable has no valid .daedalus footer",
        ));
    }
    let payload_offset = u64::from_le_bytes(
        footer[16..24]
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "payload_offset truncated"))?,
    ) as usize;
    let meta_offset = u64::from_le_bytes(
        footer[72..80]
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "meta_offset truncated"))?,
    ) as usize;
    if payload_offset > meta_offset || meta_offset > base.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "footer offsets are out of bounds",
        ));
    }
    Ok((payload_offset, meta_offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal in-memory `.daedalus` layout with the given payload.
    fn build_base(stub: &[u8], payload: &[u8], meta: &[u8]) -> Vec<u8> {
        let payload_offset = stub.len();
        let meta_offset = payload_offset + payload.len();
        let mut base = Vec::new();
        base.extend_from_slice(stub);
        base.extend_from_slice(payload);
        base.extend_from_slice(meta);

        // Footer: last 92 bytes, with magic + payload/meta offsets.
        let mut footer = [0u8; 92];
        footer[5..10].copy_from_slice(&[0x58, 0x42, 0x49, 0x4E, 0x01]); // "XBIN\x01"
        footer[10] = 5; // format version
        footer[16..24].copy_from_slice(&(payload_offset as u64).to_le_bytes());
        footer[24..32].copy_from_slice(&(payload.len() as u64).to_le_bytes());
        footer[72..80].copy_from_slice(&(meta_offset as u64).to_le_bytes());
        footer[88..92].copy_from_slice(&crate::format::FOOTER_MAGIC.to_le_bytes());
        base.extend_from_slice(&footer);
        base
    }

    #[test]
    fn daedalus_stitcher_replaces_payload_region() {
        let stub = b"ELF-stub-here";
        let old_payload = b"old-squashfs";
        let meta = b"{\"runtime\":\"python\"}";
        let base = build_base(stub, old_payload, meta);

        let new_blocks = vec![b"block-a".to_vec(), b"block-b".to_vec()];
        let mut output = Vec::new();
        DaedalusStitcher
            .assemble(&base, &new_blocks, &mut output)
            .unwrap();

        // Header before payload is preserved, new blocks inserted, tail intact.
        let expected: Vec<u8> =
            [stub, &b"block-ablock-b"[..], meta, &base[base.len() - 92..]].concat();
        assert_eq!(output, expected);
        assert!(!output.windows(old_payload.len()).any(|w| w == old_payload));
    }

    #[test]
    fn daedalus_stitcher_rejects_invalid_base() {
        let mut output = Vec::new();
        assert!(DaedalusStitcher
            .assemble(b"not a daedalus", &[], &mut output)
            .is_err());
    }

    #[test]
    fn daedalus_stitcher_rejects_truncated_base() {
        let base = build_base(b"stub", b"payload", b"meta");
        let mut output = Vec::new();
        assert!(DaedalusStitcher
            .assemble(&base[..base.len() - 40], &[], &mut output)
            .is_err());
    }

    #[test]
    fn daedalus_stitcher_rejects_out_of_bounds_offsets() {
        let base = build_base(b"stub", b"payload", b"meta");
        let mut corrupted = base.clone();
        let footer_len = 92;
        let footer_start = corrupted.len() - footer_len;
        // Point meta_offset past the end of the file.
        corrupted[footer_start + 72..footer_start + 80].copy_from_slice(&(u64::MAX).to_le_bytes());
        let mut output = Vec::new();
        assert!(DaedalusStitcher
            .assemble(&corrupted, &[], &mut output)
            .is_err());
    }
}
