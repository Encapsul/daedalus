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
        self.payload_sha256
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
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
