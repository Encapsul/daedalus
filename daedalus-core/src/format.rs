//! `.daedalus` format parser — see docs/src/reference/format.md.
//!
//! Shared between the launcher (stub) and the build tool.
//! Footer versions: v1-v5. See format docs for layout details.

use std::io::{self, Read, Seek, SeekFrom};

pub const MAGIC: &[u8; 5] = b"ERE\x01\x00";
pub const FOOTER_MAGIC: u32 = 0xBEEF_CAFE;
pub const FORMAT_VERSION: u8 = 5;

pub const V2_FOOTER_SIZE: u64 = 84;
pub const V3_FOOTER_SIZE: u64 = 92;

/// Fixed `SISR` access block placed immediately before the standard footer.
pub const SISR_FOOTER_EXT_SIZE: usize = 110;

pub const CRYPTO_NONE: u64 = 0x00;

pub const PAYLOAD_FORMAT_SQUASHFS: &str = "squashfs";

pub const FLAG_SIGNED: u8 = 0x01;
pub const FLAG_ENCRYPTED: u8 = 0x08;
/// Set when the file carries a `SISR` footer extension + delta manifest.
pub const FLAG_SISR: u8 = 0x04;

pub const ARCH_X86_64: u8 = 0x01;
pub const ARCH_AARCH64: u8 = 0x02;

pub const SIG_BLOCK_SIZE: usize = 68;
pub const SIG_BLOCK_SIZE_FIELD: usize = 4;
/// Ed25519 signature length, derived from the fixed block layout.
pub const SIG_LEN: usize = SIG_BLOCK_SIZE - SIG_BLOCK_SIZE_FIELD;

/// Fixed footer at the very end of a .daedalus file.
#[derive(Debug)]
pub struct Footer {
    pub format_version: u8,
    pub arch: u8,
    pub flags: u8,
    pub payload_offset: u64,
    pub payload_csize: u64,
    pub payload_usize: u64,
    pub payload_sha256: [u8; 32],
    pub meta_offset: u64,
    pub meta_size: u64,
    pub sig_offset: u64,
}

impl Footer {
    /// `is_signed` - check whether signed.
    ///
    /// Description:
    ///
    /// Return: true or false
    pub fn is_signed(&self) -> bool {
        self.flags & FLAG_SIGNED != 0
    }

    /// Size of the standard footer block for this format version.
    pub fn footer_size(&self) -> u64 {
        if self.format_version >= 3 {
            V3_FOOTER_SIZE
        } else {
            V2_FOOTER_SIZE
        }
    }

    /// Whether the file embeds a `SISR` extension and delta manifest.
    pub fn has_sisr(&self) -> bool {
        self.flags & FLAG_SISR != 0
    }

    /// `is_encrypted` - check whether encrypted.
    ///
    /// Description:
    ///
    /// Return: true or false
    pub fn is_encrypted(&self) -> bool {
        self.flags & FLAG_ENCRYPTED != 0
    }

    pub fn read_from<R: Read + Seek>(r: &mut R) -> io::Result<Footer> {
        let total = r.seek(SeekFrom::End(0))?;

        if total >= V3_FOOTER_SIZE {
            r.seek(SeekFrom::End(-(V3_FOOTER_SIZE as i64)))?;
            let mut buf = [0u8; V3_FOOTER_SIZE as usize];
            r.read_exact(&mut buf)?;

            if &buf[8..13] == MAGIC && buf[13] >= 3 {
                let sig_offset = u64_le(&buf[0..8])?;
                return Self::parse(&buf[8..], sig_offset);
            }
        }

        if total >= V2_FOOTER_SIZE {
            r.seek(SeekFrom::End(-(V2_FOOTER_SIZE as i64)))?;
            let mut buf = [0u8; V2_FOOTER_SIZE as usize];
            r.read_exact(&mut buf)?;

            if &buf[0..5] == MAGIC {
                return Self::parse(&buf, 0);
            }
            return Err(err("bad magic: not a .daedalus file"));
        }

        Err(err("file too small to be a .daedalus"))
    }

    /// `parse` - parse.
    /// `@buf`: buffer
    /// `@sig_offset`: sig offset
    /// `@io`: io
    ///
    /// Description:
    ///
    /// Return: Result containing `io::Result<Footer>`
    fn parse(buf: &[u8], sig_offset: u64) -> io::Result<Footer> {
        let footer_magic = u32::from_le_bytes(
            buf[80..84]
                .try_into()
                .map_err(|_| err("truncated footer magic"))?,
        );
        if footer_magic != FOOTER_MAGIC {
            return Err(err("bad footer sentinel"));
        }
        let format_version = buf[5];
        if format_version > FORMAT_VERSION {
            return Err(err("unsupported .daedalus format version"));
        }

        let mut payload_sha256 = [0u8; 32];
        payload_sha256.copy_from_slice(&buf[32..64]);

        Ok(Footer {
            format_version,
            arch: buf[6],
            flags: buf[7],
            payload_offset: u64_le(&buf[8..16])?,
            payload_csize: u64_le(&buf[16..24])?,
            payload_usize: u64_le(&buf[24..32])?,
            payload_sha256,
            meta_offset: u64_le(&buf[64..72])?,
            meta_size: u64_le(&buf[72..80])?,
            sig_offset,
        })
    }

    /// `pack` - pack.
    ///
    /// Description:
    ///
    /// Return: the `[u8; 84]`
    pub fn pack(&self) -> [u8; 84] {
        let mut buf = [0u8; 84];
        buf[0..5].copy_from_slice(MAGIC);
        buf[5] = self.format_version;
        buf[6] = self.arch;
        buf[7] = self.flags;
        buf[8..16].copy_from_slice(&self.payload_offset.to_le_bytes());
        buf[16..24].copy_from_slice(&self.payload_csize.to_le_bytes());
        buf[24..32].copy_from_slice(&self.payload_usize.to_le_bytes());
        buf[32..64].copy_from_slice(&self.payload_sha256);
        buf[64..72].copy_from_slice(&self.meta_offset.to_le_bytes());
        buf[72..80].copy_from_slice(&self.meta_size.to_le_bytes());
        buf[80..84].copy_from_slice(&FOOTER_MAGIC.to_le_bytes());
        buf
    }

    /// Full on-disk footer representation (v3+): the 8-byte `sig_offset`
    /// prefix followed by the 84-byte core, byte-identical to what a signed
    /// file stores at EOF. Used as the signature digest input — the footer
    /// decides whether the signature is consulted at all (version + flags),
    /// so a signature over payload‖meta alone would let an attacker downgrade
    /// the file to v2 and have it skipped.
    pub fn pack_full(&self) -> [u8; 92] {
        let mut buf = [0u8; V3_FOOTER_SIZE as usize];
        buf[0..8].copy_from_slice(&self.sig_offset.to_le_bytes());
        buf[8..].copy_from_slice(&self.pack());
        buf
    }

    /// `sha256_hex` - sha256 hex.
    ///
    /// Description:
    ///
    /// Return: the `resulting` string
    pub fn sha256_hex(&self) -> String {
        self.payload_sha256
            .iter()
            .fold(String::with_capacity(64), |mut s, b| {
                use std::fmt::Write;
                let _ = write!(s, "{b:02x}");
                s
            })
    }
}

/// `u64_le` - u64 le.
/// `@b`: b
/// `@io`: io
///
/// Description:
///
/// Return: Result containing `io::Result<u64>`
fn u64_le(b: &[u8]) -> io::Result<u64> {
    Ok(u64::from_le_bytes(
        b.try_into().map_err(|_| err("truncated u64 field"))?,
    ))
}

pub fn read_at<R: Read + Seek>(f: &mut R, off: u64, len: usize) -> io::Result<Vec<u8>> {
    // `off` and `len` originate from a `.daedalus` footer (untrusted input). A
    // malicious footer can set `len` to `u64::MAX`, which the call sites fold to
    // `usize::MAX` via `as usize`; allocating `vec![0u8; len]` up-front would then
    // OOM/abort the process *before* signature or SHA-256 verification. Bound the
    // allocation to the real stream length and reject out-of-range reads.
    let stream_len = {
        let cur = f.stream_position()?;
        let end = f.seek(SeekFrom::End(0))?;
        f.seek(SeekFrom::Start(cur))?;
        end
    };
    let end = off
        .checked_add(len as u64)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "offset+len overflows u64"))?;
    if end > stream_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "footer field exceeds file size",
        ));
    }
    let mut buf = vec![0u8; len];
    f.seek(SeekFrom::Start(off))?;
    f.read_exact(&mut buf)?;
    Ok(buf)
}

/// `err` - err.
/// `@msg`: message
/// `@io`: io
///
/// Description:
///
/// Return: the `std::io::Error`
fn err(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[allow(clippy::too_many_arguments)]
    /// `build_v2_footer` - build v2 footer.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn build_v2_footer(
        version: u8,
        arch: u8,
        flags: u8,
        payload_offset: u64,
        payload_csize: u64,
        payload_usize: u64,
        sha256: [u8; 32],
        meta_offset: u64,
        meta_size: u64,
    ) -> Vec<u8> {
        let mut buf = vec![0u8; V2_FOOTER_SIZE as usize];
        buf[0..5].copy_from_slice(MAGIC);
        buf[5] = version;
        buf[6] = arch;
        buf[7] = flags;
        buf[8..16].copy_from_slice(&payload_offset.to_le_bytes());
        buf[16..24].copy_from_slice(&payload_csize.to_le_bytes());
        buf[24..32].copy_from_slice(&payload_usize.to_le_bytes());
        buf[32..64].copy_from_slice(&sha256);
        buf[64..72].copy_from_slice(&meta_offset.to_le_bytes());
        buf[72..80].copy_from_slice(&meta_size.to_le_bytes());
        buf[80..84].copy_from_slice(&FOOTER_MAGIC.to_le_bytes());
        buf
    }

    #[allow(clippy::too_many_arguments)]
    /// `build_v3_footer` - build v3 footer.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn build_v3_footer(
        version: u8,
        arch: u8,
        flags: u8,
        payload_offset: u64,
        payload_csize: u64,
        payload_usize: u64,
        sha256: [u8; 32],
        meta_offset: u64,
        meta_size: u64,
        sig_offset: u64,
    ) -> Vec<u8> {
        let mut prefix = vec![0u8; 8];
        prefix[0..8].copy_from_slice(&sig_offset.to_le_bytes());
        let v2 = build_v2_footer(
            version,
            arch,
            flags,
            payload_offset,
            payload_csize,
            payload_usize,
            sha256,
            meta_offset,
            meta_size,
        );
        prefix.extend_from_slice(&v2);
        prefix
    }

    #[test]
    /// `constants_are_correct` - constants are correct.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn constants_are_correct() {
        assert_eq!(MAGIC, b"ERE\x01\x00");
        assert_eq!(FOOTER_MAGIC, 0xBEEF_CAFE);
        assert_eq!(FORMAT_VERSION, 5);
        assert_eq!(V2_FOOTER_SIZE, 84);
        assert_eq!(V3_FOOTER_SIZE, 92);
    }

    #[test]
    /// `parse_v2_footer` - parse v2 footer.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn parse_v2_footer() {
        let sha = [0xABu8; 32];
        let raw = build_v2_footer(3, 0x3C, 0x01, 1024, 512, 2048, sha, 2560, 128);
        let f = Footer::parse(&raw, 0).unwrap();
        assert_eq!(f.format_version, 3);
        assert_eq!(f.arch, 0x3C);
        assert_eq!(f.flags, 0x01);
        assert_eq!(f.payload_offset, 1024);
        assert_eq!(f.payload_csize, 512);
        assert_eq!(f.payload_usize, 2048);
        assert_eq!(f.payload_sha256, sha);
        assert_eq!(f.meta_offset, 2560);
        assert_eq!(f.meta_size, 128);
        assert_eq!(f.sig_offset, 0);
    }

    #[test]
    /// `parse_v3_footer_with_sig` - parse v3 footer with sig.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn parse_v3_footer_with_sig() {
        let sha = [0x42u8; 32];
        let raw = build_v3_footer(5, 0x86, 0x03, 4096, 1024, 8192, sha, 5120, 256, 9999);
        let f = Footer::parse(&raw[8..], 9999).unwrap();
        assert_eq!(f.format_version, 5);
        assert_eq!(f.arch, 0x86);
        assert_eq!(f.flags, 0x03);
        assert_eq!(f.payload_offset, 4096);
        assert_eq!(f.sig_offset, 9999);
    }

    #[test]
    /// `bad_footer_magic_returns_err` - bad footer magic returns err.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn bad_footer_magic_returns_err() {
        let mut raw = build_v2_footer(3, 0, 0, 0, 0, 0, [0; 32], 0, 0);
        raw[80..84].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        let result = Footer::parse(&raw, 0);
        assert!(result.is_err());
    }

    #[test]
    /// `unsupported_version_returns_err` - unsupported version returns err.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn unsupported_version_returns_err() {
        let raw = build_v2_footer(99, 0, 0, 0, 0, 0, [0; 32], 0, 0);
        let result = Footer::parse(&raw, 0);
        assert!(result.is_err());
    }

    #[test]
    /// `is_signed_checks_flag` - check whether signed checks flag.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn is_signed_checks_flag() {
        let mut f = Footer {
            format_version: 5,
            arch: 0,
            flags: 0,
            payload_offset: 0,
            payload_csize: 0,
            payload_usize: 0,
            payload_sha256: [0; 32],
            meta_offset: 0,
            meta_size: 0,
            sig_offset: 0,
        };
        assert!(!f.is_signed());
        f.flags = FLAG_SIGNED;
        assert!(f.is_signed());
    }

    #[test]
    /// `pack_full_roundtrips_v3_footer` - pack full roundtrips v3 footer.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn pack_full_roundtrips_v3_footer() {
        let sha = [0xDDu8; 32];
        let f = Footer {
            format_version: 5,
            arch: 0x86,
            flags: FLAG_SIGNED,
            payload_offset: 4096,
            payload_csize: 1024,
            payload_usize: 8192,
            payload_sha256: sha,
            meta_offset: 5120,
            meta_size: 256,
            sig_offset: 5376,
        };
        let raw = f.pack_full();
        assert_eq!(raw.len(), V3_FOOTER_SIZE as usize);
        let mut data = vec![0u8; f.payload_offset as usize + 100];
        data.extend_from_slice(&raw);
        let parsed = Footer::read_from(&mut Cursor::new(data)).unwrap();
        assert_eq!(parsed.format_version, f.format_version);
        assert_eq!(parsed.flags, f.flags);
        assert_eq!(parsed.sig_offset, f.sig_offset);
        assert_eq!(parsed.payload_sha256, f.payload_sha256);
    }

    #[test]
    /// `sha256_hex_is_correct` - sha256 hex is correct.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn sha256_hex_is_correct() {
        let sha = [
            0x00, 0x01, 0x0A, 0xFF, 0xAB, 0xCD, 0xEF, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE,
            0xF0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD,
            0xEE, 0xFF, 0x00, 0x12,
        ];
        let f = Footer {
            format_version: 5,
            arch: 0,
            flags: 0,
            payload_offset: 0,
            payload_csize: 0,
            payload_usize: 0,
            payload_sha256: sha,
            meta_offset: 0,
            meta_size: 0,
            sig_offset: 0,
        };
        let hex = f.sha256_hex();
        assert_eq!(hex.len(), 64);
        assert!(hex.starts_with("00010aff"));
    }

    #[test]
    /// `read_from_v2_file` - read from v2 file.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn read_from_v2_file() {
        let sha = [0xBBu8; 32];
        let footer = build_v2_footer(3, 0x3C, 0, 100, 50, 200, sha, 160, 30);
        let mut data = vec![0u8; 200];
        data.extend_from_slice(&footer);
        let mut cursor = Cursor::new(data);
        let f = Footer::read_from(&mut cursor).unwrap();
        assert_eq!(f.format_version, 3);
        assert_eq!(f.payload_offset, 100);
        assert_eq!(f.sig_offset, 0);
    }

    #[test]
    /// `read_from_v3_file` - read from v3 file.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn read_from_v3_file() {
        let sha = [0xCCu8; 32];
        let mut data = vec![0xBB; 512];
        let footer = build_v3_footer(5, 0x86, 0x01, 200, 100, 400, sha, 320, 60, 7777);
        data.extend_from_slice(&footer);
        let mut cursor = Cursor::new(data);
        let f = Footer::read_from(&mut cursor).unwrap();
        assert_eq!(f.format_version, 5);
        assert_eq!(f.sig_offset, 7777);
    }

    #[test]
    /// `read_from_too_small` - read from too small.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn read_from_too_small() {
        let mut data = vec![0u8; 10];
        let mut cursor = Cursor::new(&mut data);
        let result = Footer::read_from(&mut cursor);
        assert!(result.is_err());
    }

    #[test]
    /// `read_from_bad_magic` - read from bad magic.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn read_from_bad_magic() {
        let mut data = vec![0u8; 300];
        let footer = build_v2_footer(3, 0, 0, 0, 0, 0, [0; 32], 0, 0);
        let start = 300 - V2_FOOTER_SIZE as usize;
        data[start..].copy_from_slice(&footer);
        data[start..start + 5].copy_from_slice(b"BAD\x01\x02");
        let mut cursor = Cursor::new(data);
        let result = Footer::read_from(&mut cursor);
        assert!(result.is_err());
    }

    #[test]
    /// `read_at_reads_correct_bytes` - read at reads correct bytes.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn read_at_reads_correct_bytes() {
        let data = vec![0, 0, 0, 42, 43, 44, 0, 0];
        let mut cursor = Cursor::new(data);
        let result = read_at(&mut cursor, 3, 3).unwrap();
        assert_eq!(result, vec![42, 43, 44]);
    }

    #[test]
    /// `read_at_rejects_oversized_len_without_allocating` - read at rejects oversized len without allocating.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn read_at_rejects_oversized_len_without_allocating() {
        // Simulates a malicious footer whose csize/len field is u64::MAX, which
        // the call sites fold to `usize::MAX` via `as usize`. read_at must reject
        // it via the file-size bound instead of allocating (and aborting).
        let data = b"daedalus";
        let mut cursor = Cursor::new(data);
        let result = read_at(&mut cursor, 0, usize::MAX);
        assert!(result.is_err(), "oversized read must be rejected");
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use std::io::Cursor;

    proptest! {
        #[test]
        /// `read_from_arbitrary_bytes_never_panics` - read from arbitrary bytes never panics.
        ///
        /// Description:
        ///
        /// Return: nothing
        fn read_from_arbitrary_bytes_never_panics(
            buf in prop::collection::vec(any::<u8>(), 0..4096),
        ) {
            let _ = Footer::read_from(&mut Cursor::new(&buf));
        }

        #[test]
        /// `pack_roundtrips` - pack roundtrips.
        ///
        /// Description:
        ///
        /// Return: nothing
        fn pack_roundtrips(
            format_version in 1u8..=FORMAT_VERSION,
            arch in any::<u8>(),
            flags in any::<u8>(),
            payload_offset in any::<u64>(),
            payload_csize in any::<u64>(),
            payload_usize in any::<u64>(),
            payload_sha256 in prop::array::uniform32(any::<u8>()),
            meta_offset in any::<u64>(),
            meta_size in any::<u64>(),
        ) {
            let footer = Footer {
                format_version,
                arch,
                flags,
                payload_offset,
                payload_csize,
                payload_usize,
                payload_sha256,
                meta_offset,
                meta_size,
                sig_offset: 0,
            };
            let parsed = Footer::read_from(&mut Cursor::new(&footer.pack())).unwrap();
            prop_assert_eq!(parsed.format_version, footer.format_version);
            prop_assert_eq!(parsed.arch, footer.arch);
            prop_assert_eq!(parsed.flags, footer.flags);
            prop_assert_eq!(parsed.payload_offset, footer.payload_offset);
            prop_assert_eq!(parsed.payload_csize, footer.payload_csize);
            prop_assert_eq!(parsed.payload_usize, footer.payload_usize);
            prop_assert_eq!(parsed.payload_sha256, footer.payload_sha256);
            prop_assert_eq!(parsed.meta_offset, footer.meta_offset);
            prop_assert_eq!(parsed.meta_size, footer.meta_size);
            prop_assert_eq!(parsed.sig_offset, 0);
        }
    }
}
