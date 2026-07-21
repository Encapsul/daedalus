//! `.xbin` format parser — see docs/src/reference/format.md.
//!
//! Shared between the launcher (stub) and the build tool.
//! Footer versions: v1-v5. See format docs for layout details.

use std::io::{self, Read, Seek, SeekFrom};

pub const MAGIC: &[u8; 5] = b"XBIN\x01";
pub const FOOTER_MAGIC: u32 = 0xBEEF_CAFE;
pub const FORMAT_VERSION: u8 = 5;

pub const V2_FOOTER_SIZE: u64 = 84;
pub const V3_FOOTER_SIZE: u64 = 92;

pub const CRYPTO_NONE: u64 = 0x00;
pub const CRYPTO_AES_256_GCM: u64 = 0x01;

pub const PAYLOAD_FORMAT_ZSTD_TAR: &str = "zstd-tar";
pub const PAYLOAD_FORMAT_SQUASHFS: &str = "squashfs";

pub const FLAG_SIGNED: u8 = 0x01;
pub const FLAG_ENCRYPTED: u8 = 0x02;

/// Fixed footer at the very end of a .xbin file.
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
    pub fn crypto_suite(&self) -> u64 {
        if self.format_version >= 4 {
            self.payload_usize
        } else {
            CRYPTO_NONE
        }
    }

    pub fn is_signed(&self) -> bool {
        self.flags & FLAG_SIGNED != 0
    }

    pub fn read_from<R: Read + Seek>(r: &mut R) -> io::Result<Footer> {
        let total = r.seek(SeekFrom::End(0))?;

        if total >= V3_FOOTER_SIZE {
            r.seek(SeekFrom::End(-(V3_FOOTER_SIZE as i64)))?;
            let mut buf = [0u8; V3_FOOTER_SIZE as usize];
            r.read_exact(&mut buf)?;

            if &buf[8..13] == MAGIC && buf[13] >= 3 {
                let sig_offset = u64_le(&buf[0..8]);
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
            return Err(err("bad magic: not a .xbin file"));
        }

        Err(err("file too small to be a .xbin"))
    }

    fn parse(buf: &[u8], sig_offset: u64) -> io::Result<Footer> {
        let footer_magic = u32::from_le_bytes(buf[80..84].try_into().unwrap());
        if footer_magic != FOOTER_MAGIC {
            return Err(err("bad footer sentinel"));
        }
        let format_version = buf[5];
        if format_version > FORMAT_VERSION {
            return Err(err("unsupported .xbin format version"));
        }

        let mut payload_sha256 = [0u8; 32];
        payload_sha256.copy_from_slice(&buf[32..64]);

        Ok(Footer {
            format_version,
            arch: buf[6],
            flags: buf[7],
            payload_offset: u64_le(&buf[8..16]),
            payload_csize: u64_le(&buf[16..24]),
            payload_usize: u64_le(&buf[24..32]),
            payload_sha256,
            meta_offset: u64_le(&buf[64..72]),
            meta_size: u64_le(&buf[72..80]),
            sig_offset,
        })
    }

    pub fn sha256_hex(&self) -> String {
        self.payload_sha256.iter().fold(String::with_capacity(64), |mut s, b| {
            use std::fmt::Write;
            let _ = write!(s, "{b:02x}");
            s
        })
    }
}

fn u64_le(b: &[u8]) -> u64 {
    u64::from_le_bytes(b.try_into().unwrap())
}

pub fn read_at<R: Read + Seek>(f: &mut R, off: u64, len: usize) -> io::Result<Vec<u8>> {
    f.seek(SeekFrom::Start(off))?;
    let mut buf = vec![0u8; len];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

fn err(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

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
            version, arch, flags, payload_offset, payload_csize, payload_usize, sha256,
            meta_offset, meta_size,
        );
        prefix.extend_from_slice(&v2);
        prefix
    }

    #[test]
    fn constants_are_correct() {
        assert_eq!(MAGIC, b"XBIN\x01");
        assert_eq!(FOOTER_MAGIC, 0xBEEF_CAFE);
        assert_eq!(FORMAT_VERSION, 5);
        assert_eq!(V2_FOOTER_SIZE, 84);
        assert_eq!(V3_FOOTER_SIZE, 92);
    }

    #[test]
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
    fn bad_footer_magic_returns_err() {
        let mut raw = build_v2_footer(3, 0, 0, 0, 0, 0, [0; 32], 0, 0);
        raw[80..84].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        let result = Footer::parse(&raw, 0);
        assert!(result.is_err());
    }

    #[test]
    fn unsupported_version_returns_err() {
        let raw = build_v2_footer(99, 0, 0, 0, 0, 0, [0; 32], 0, 0);
        let result = Footer::parse(&raw, 0);
        assert!(result.is_err());
    }

    #[test]
    fn crypto_suite_v3_returns_none() {
        let f = Footer {
            format_version: 3,
            arch: 0,
            flags: 0,
            payload_offset: 0,
            payload_csize: 0,
            payload_usize: 12345,
            payload_sha256: [0; 32],
            meta_offset: 0,
            meta_size: 0,
            sig_offset: 0,
        };
        assert_eq!(f.crypto_suite(), CRYPTO_NONE);
    }

    #[test]
    fn crypto_suite_v4_returns_usize() {
        let f = Footer {
            format_version: 4,
            arch: 0,
            flags: 0,
            payload_offset: 0,
            payload_csize: 0,
            payload_usize: 99999,
            payload_sha256: [0; 32],
            meta_offset: 0,
            meta_size: 0,
            sig_offset: 0,
        };
        assert_eq!(f.crypto_suite(), 99999);
    }

    #[test]
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
        f.flags = FLAG_SIGNED | FLAG_ENCRYPTED;
        assert!(f.is_signed());
    }

    #[test]
    fn is_encrypted_checks_flag() {
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
        assert_eq!(f.flags & FLAG_ENCRYPTED, 0);
        f.flags = FLAG_ENCRYPTED;
        assert!(f.flags & FLAG_ENCRYPTED != 0);
    }

    #[test]
    fn sha256_hex_is_correct() {
        let sha = [0x00, 0x01, 0x0A, 0xFF, 0xAB, 0xCD, 0xEF, 0x12,
                    0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0, 0x11,
                    0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99,
                    0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x00, 0x12];
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
    fn read_from_too_small() {
        let mut data = vec![0u8; 10];
        let mut cursor = Cursor::new(&mut data);
        let result = Footer::read_from(&mut cursor);
        assert!(result.is_err());
    }

    #[test]
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
    fn read_at_reads_correct_bytes() {
        let data = vec![0, 0, 0, 42, 43, 44, 0, 0];
        let mut cursor = Cursor::new(data);
        let result = read_at(&mut cursor, 3, 3).unwrap();
        assert_eq!(result, vec![42, 43, 44]);
    }
}
