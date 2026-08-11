//! `SISR` footer extension — the fixed access block that locates and binds
//! the embedded delta manifest.
//!
//! The extension is placed immediately *before* the standard footer, so
//! legacy decoders (which read backwards from EOF) never see it and keep
//! decoding old files byte-for-byte. New decoders gate on
//! [`format::FLAG_SISR`] in the standard footer: the bit set means the
//! extension exists, the bit clear means the file predates `SISR`.

use std::io::{self, Read, Seek, SeekFrom};

use crate::format;
use crate::manifest::DeltaManifest;

/// Byte size of the fixed `SISR` footer extension.
pub const SIZE: usize = format::SISR_FOOTER_EXT_SIZE;

/// Maximum allowed manifest size (4 MiB). Prevents OOM from malicious
/// binaries claiming an enormous chunk table.
pub const MAX_MANIFEST_SIZE: usize = 4 * 1024 * 1024;

/// Version of the `SISR` extension schema understood by this crate.
pub const SISR_VERSION: u16 = 1;

/// Fixed access block between the app metadata and the standard footer.
///
/// Not `#[repr(C, packed)]`: `xbin-core` is `unsafe`-free, so fields are
/// moved through explicit little-endian serialization instead of transmutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SisrFooterExt {
    /// `SISR` extension schema version; `0` marks the block as absent.
    pub sisr_version: u16,
    /// Absolute file offset of the serialized [`DeltaManifest`].
    pub chunk_table_offset: u64,
    /// Byte length of the serialized [`DeltaManifest`].
    pub chunk_table_len: u32,
    /// Merkle root over the payload chunk hashes (binds the manifest).
    pub merkle_root: [u8; 32],
    /// Ed25519 signature over the serialized [`DeltaManifest`].
    pub signature: [u8; 64],
}

impl SisrFooterExt {
    /// Whether the block marks a real `SISR` section (`sisr_version != 0`).
    pub fn is_present(&self) -> bool {
        self.sisr_version != 0
    }

    /// Serializes the extension into its fixed-size byte form.
    pub fn pack(&self) -> [u8; SIZE] {
        let mut buf = [0u8; SIZE];
        buf[0..2].copy_from_slice(&self.sisr_version.to_le_bytes());
        buf[2..10].copy_from_slice(&self.chunk_table_offset.to_le_bytes());
        buf[10..14].copy_from_slice(&self.chunk_table_len.to_le_bytes());
        buf[14..46].copy_from_slice(&self.merkle_root);
        buf[46..110].copy_from_slice(&self.signature);
        buf
    }

    /// Parses the extension from exactly [`SIZE`] bytes.
    pub fn parse(buf: &[u8]) -> io::Result<Self> {
        let bytes = buf
            .get(..SIZE)
            .ok_or_else(|| err("truncated SISR footer extension"))?;
        Ok(Self {
            sisr_version: u16::from_le_bytes(fixed(&bytes[0..2])?),
            chunk_table_offset: u64::from_le_bytes(fixed(&bytes[2..10])?),
            chunk_table_len: u32::from_le_bytes(fixed(&bytes[10..14])?),
            merkle_root: fixed(&bytes[14..46])?,
            signature: fixed(&bytes[46..110])?,
        })
    }

    /// Reads the extension immediately before the standard footer.
    ///
    /// `footer_size` is 84 for v2 footers and 92 for v3+. Returns `Ok(None)`
    /// when the file is too small to hold an extension or the block is marked
    /// absent (`sisr_version == 0`).
    pub fn read_from<R: Read + Seek>(
        r: &mut R,
        file_len: u64,
        footer_size: u64,
    ) -> io::Result<Option<Self>> {
        let ext_start = file_len
            .checked_sub(footer_size)
            .and_then(|p| p.checked_sub(SIZE as u64));
        let Some(ext_start) = ext_start else {
            return Ok(None);
        };
        r.seek(SeekFrom::Start(ext_start))?;
        let mut buf = [0u8; SIZE];
        r.read_exact(&mut buf)?;
        let ext = Self::parse(&buf)?;
        Ok(ext.is_present().then_some(ext))
    }
}

/// Reads the `SISR` extension and delta manifest from a `.xbin` stream.
///
/// Returns `Ok(None)` for files predating `SISR` (the `FLAG_SISR` bit is
/// clear), so legacy binaries decode transparently. Out-of-bounds offsets, a
/// truncated or malformed manifest, or a set flag with a missing extension
/// all fail.
pub fn read_sisr<R: Read + Seek>(r: &mut R) -> io::Result<Option<(SisrFooterExt, DeltaManifest)>> {
    let file_len = r.seek(SeekFrom::End(0))?;
    let footer = format::Footer::read_from(r)?;
    if !footer.has_sisr() {
        return Ok(None);
    }
    let footer_size = footer.footer_size();
    let ext = SisrFooterExt::read_from(r, file_len, footer_size)?
        .ok_or_else(|| err("SISR flag set but SISR footer extension is absent"))?;
    let ext_start = file_len
        .checked_sub(footer_size)
        .and_then(|p| p.checked_sub(SIZE as u64))
        .ok_or_else(|| err("SISR extension out of file bounds"))?;
    let len = usize::try_from(ext.chunk_table_len)
        .map_err(|_| err("SISR chunk table length overflow"))?;
    // Reject oversized manifests before any bounds math or allocation.
    if len > MAX_MANIFEST_SIZE {
        return Err(err("SISR manifest exceeds maximum allowed size"));
    }
    let table_end = u64::from(ext.chunk_table_len)
        .checked_add(ext.chunk_table_offset)
        .ok_or_else(|| err("SISR chunk table offset overflow"))?;
    if table_end > ext_start {
        return Err(err("SISR chunk table out of file bounds"));
    }
    r.seek(SeekFrom::Start(ext.chunk_table_offset))?;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    let manifest = DeltaManifest::parse(&buf)?;
    Ok(Some((ext, manifest)))
}

fn fixed<const N: usize>(b: &[u8]) -> io::Result<[u8; N]> {
    b.try_into()
        .map_err(|_| err("truncated SISR footer extension"))
}

fn err(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn ext() -> SisrFooterExt {
        SisrFooterExt {
            sisr_version: SISR_VERSION,
            chunk_table_offset: 1234,
            chunk_table_len: 92,
            merkle_root: [0xAB; 32],
            signature: [0xCD; 64],
        }
    }

    fn v3_footer(flags: u8) -> [u8; 92] {
        let core = format::Footer {
            format_version: 5,
            arch: 0x3C,
            flags,
            payload_offset: 100,
            payload_csize: 50,
            payload_usize: 200,
            payload_sha256: [0xBB; 32],
            meta_offset: 300,
            meta_size: 40,
            sig_offset: 0,
        }
        .pack();
        let mut full = [0u8; 92];
        full[0..8].copy_from_slice(&0u64.to_le_bytes());
        full[8..].copy_from_slice(&core);
        full
    }

    fn footer_size_hint() -> u64 {
        format::V3_FOOTER_SIZE
    }

    #[test]
    fn size_is_110_bytes() {
        assert_eq!(SIZE, 110);
        assert_eq!(SisrFooterExt::pack(&ext()).len(), 110);
    }

    #[test]
    fn header_overhead_stays_under_4kib() {
        let manifest = DeltaManifest {
            version: crate::manifest::VERSION,
            payload_len: 100 << 20,
            chunks: (0u8..100)
                .map(|i| crate::manifest::ChunkEntry {
                    hash: [i; 32],
                    length: 1024,
                })
                .collect(),
        };
        let overhead = SIZE + manifest.encoded_len();
        assert!(
            overhead < 4096,
            "SISR header overhead {overhead} exceeds 4 KiB"
        );
    }

    #[test]
    fn pack_parse_roundtrip_is_bit_exact() {
        let e = ext();
        let bytes = e.pack();
        assert_eq!(SisrFooterExt::parse(&bytes).unwrap(), e);
        assert!(e.is_present());
    }

    #[test]
    fn parse_rejects_short_buffer() {
        assert!(SisrFooterExt::parse(&[0u8; SIZE - 1]).is_err());
        assert!(SisrFooterExt::parse(&[]).is_err());
    }

    #[test]
    fn read_from_locates_ext_before_footer() {
        let mut data = vec![0u8; 92];
        data.extend_from_slice(&ext().pack());
        data.extend_from_slice(&v3_footer(format::FLAG_SISR));
        let len = data.len() as u64;
        let mut cursor = Cursor::new(data);
        let got = SisrFooterExt::read_from(&mut cursor, len, footer_size_hint())
            .unwrap()
            .unwrap();
        assert_eq!(got, ext());
    }

    #[test]
    fn read_from_returns_none_when_absent() {
        let mut e = ext();
        e.sisr_version = 0;
        let mut data = Vec::new();
        data.extend_from_slice(&e.pack());
        data.extend_from_slice(&v3_footer(0));
        let len = data.len() as u64;
        let mut cursor = Cursor::new(data);
        let got = SisrFooterExt::read_from(&mut cursor, len, footer_size_hint()).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn read_from_returns_none_for_small_files() {
        let mut cursor = Cursor::new(vec![0u8; 10]);
        let got = SisrFooterExt::read_from(&mut cursor, 10, footer_size_hint()).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn read_sisr_roundtrip_full_file() {
        let manifest = DeltaManifest {
            version: crate::manifest::VERSION,
            payload_len: 4096,
            chunks: vec![crate::manifest::ChunkEntry {
                hash: [7; 32],
                length: 4096,
            }],
        };
        let manifest_bytes = manifest.serialize();
        let e = SisrFooterExt {
            sisr_version: SISR_VERSION,
            chunk_table_offset: 1000,
            chunk_table_len: manifest_bytes.len() as u32,
            merkle_root: [0xEE; 32],
            signature: [0x11; 64],
        };
        let mut data = vec![0u8; 1000];
        data.extend_from_slice(&manifest_bytes);
        data.extend_from_slice(&e.pack());
        data.extend_from_slice(&v3_footer(format::FLAG_SISR));
        let mut cursor = Cursor::new(data);
        let (got_ext, got_manifest) = read_sisr(&mut cursor).unwrap().unwrap();
        assert_eq!(got_ext, e);
        assert_eq!(got_manifest, manifest);
    }

    #[test]
    fn read_sisr_returns_none_for_legacy_file() {
        let mut data = vec![0x00; 500];
        data.extend_from_slice(&v3_footer(0));
        let mut cursor = Cursor::new(data);
        assert!(read_sisr(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn read_sisr_preserves_legacy_footer_semantics() {
        let manifest = DeltaManifest {
            version: crate::manifest::VERSION,
            payload_len: 0,
            chunks: vec![],
        };
        let manifest_bytes = manifest.serialize();
        let e = SisrFooterExt {
            sisr_version: SISR_VERSION,
            chunk_table_offset: 1000,
            chunk_table_len: manifest_bytes.len() as u32,
            merkle_root: [0; 32],
            signature: [0; 64],
        };
        let mut data = vec![0u8; 1000];
        data.extend_from_slice(&manifest_bytes);
        data.extend_from_slice(&e.pack());
        data.extend_from_slice(&v3_footer(format::FLAG_SISR));
        let mut cursor = Cursor::new(data);
        let footer = format::Footer::read_from(&mut cursor).unwrap();
        assert!(footer.has_sisr());
        assert_eq!(footer.payload_offset, 100);
        assert_eq!(footer.meta_offset, 300);
        assert_eq!(footer.footer_size(), format::V3_FOOTER_SIZE);
    }

    #[test]
    fn read_sisr_rejects_out_of_bounds_chunk_table() {
        let e = SisrFooterExt {
            sisr_version: SISR_VERSION,
            chunk_table_offset: 2000,
            chunk_table_len: 20,
            merkle_root: [0; 32],
            signature: [0; 64],
        };
        let mut data = vec![0u8; 1000];
        data.extend_from_slice(&e.pack());
        data.extend_from_slice(&v3_footer(format::FLAG_SISR));
        let mut cursor = Cursor::new(data);
        assert!(read_sisr(&mut cursor).is_err());
    }

    #[test]
    fn read_sisr_rejects_offset_overflow() {
        let e = SisrFooterExt {
            sisr_version: SISR_VERSION,
            chunk_table_offset: u64::MAX - 10,
            chunk_table_len: u32::MAX,
            merkle_root: [0; 32],
            signature: [0; 64],
        };
        let mut data = vec![0u8; 1000];
        data.extend_from_slice(&e.pack());
        data.extend_from_slice(&v3_footer(format::FLAG_SISR));
        let mut cursor = Cursor::new(data);
        assert!(read_sisr(&mut cursor).is_err());
    }

    #[test]
    fn read_sisr_rejects_missing_manifest_for_set_flag() {
        let e = SisrFooterExt {
            sisr_version: SISR_VERSION,
            chunk_table_offset: 0,
            chunk_table_len: 4,
            merkle_root: [0; 32],
            signature: [0; 64],
        };
        let mut data = vec![0u8; 1000];
        data.extend_from_slice(&e.pack());
        data.extend_from_slice(&v3_footer(format::FLAG_SISR));
        let mut cursor = Cursor::new(data);
        assert!(read_sisr(&mut cursor).is_err());
    }

    #[test]
    fn read_sisr_rejects_oversized_manifest() {
        // A manifest larger than MAX_MANIFEST_SIZE must be rejected before any
        // allocation, even when the reported range sits inside the file.
        let e = SisrFooterExt {
            sisr_version: SISR_VERSION,
            chunk_table_offset: 0,
            chunk_table_len: (MAX_MANIFEST_SIZE + 1) as u32,
            merkle_root: [0; 32],
            signature: [0; 64],
        };
        let mut data = vec![0u8; 1000];
        data.extend_from_slice(&e.pack());
        data.extend_from_slice(&v3_footer(format::FLAG_SISR));
        let mut cursor = Cursor::new(data);
        let err = read_sisr(&mut cursor).unwrap_err();
        assert!(err.to_string().contains("maximum allowed size"));
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use std::io::Cursor;

    fn signature() -> impl Strategy<Value = [u8; 64]> {
        prop::collection::vec(any::<u8>(), 64).prop_map(|v| {
            let mut arr = [0u8; 64];
            arr.copy_from_slice(&v);
            arr
        })
    }

    proptest! {
        #[test]
        fn arbitrary_bytes_never_panic(
            buf in prop::collection::vec(any::<u8>(), 0..SIZE),
        ) {
            let _ = SisrFooterExt::parse(&buf);
        }

        #[test]
        fn truncated_buf_is_rejected(
            buf in prop::collection::vec(any::<u8>(), 0..SIZE),
        ) {
            if buf.len() < SIZE {
                prop_assert!(SisrFooterExt::parse(&buf).is_err());
            }
        }

        #[test]
        fn pack_roundtrips(
            sisr_version in any::<u16>(),
            chunk_table_offset in any::<u64>(),
            chunk_table_len in any::<u32>(),
            merkle_root in prop::array::uniform32(any::<u8>()),
            signature in signature(),
        ) {
            let ext = SisrFooterExt {
                sisr_version,
                chunk_table_offset,
                chunk_table_len,
                merkle_root,
                signature,
            };
            let parsed = SisrFooterExt::parse(&ext.pack()).unwrap();
            prop_assert_eq!(parsed, ext);
        }

        #[test]
        fn read_sisr_arbitrary_bytes_never_panic(
            buf in prop::collection::vec(any::<u8>(), 0..8192),
        ) {
            let _ = read_sisr(&mut Cursor::new(&buf));
        }
    }
}
