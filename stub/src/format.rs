//! Parsing du format `.xbin` — voir docs/FORMAT.md.
//!
//! Le launcher se lit lui-même via /proc/self/exe et lit le footer de 84 bytes
//! en fin de fichier pour localiser le payload et les métadonnées.

use std::io::{self, Read, Seek, SeekFrom};

pub const MAGIC: &[u8; 5] = b"XBIN\x01";
pub const FOOTER_MAGIC: u32 = 0xBEEF_CAFE;
pub const FOOTER_SIZE: u64 = 84;
pub const FORMAT_VERSION: u8 = 2;

/// Footer fixe de 84 bytes situé à la toute fin du fichier .xbin.
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
}

fn u64_le(b: &[u8]) -> u64 {
    u64::from_le_bytes(b.try_into().unwrap())
}

impl Footer {
    /// Lit et valide le footer depuis un fichier seekable.
    pub fn read_from<R: Read + Seek>(r: &mut R) -> io::Result<Footer> {
        let total = r.seek(SeekFrom::End(0))?;
        if total < FOOTER_SIZE {
            return Err(err("file too small to be a .xbin"));
        }
        r.seek(SeekFrom::End(-(FOOTER_SIZE as i64)))?;
        let mut buf = [0u8; FOOTER_SIZE as usize];
        r.read_exact(&mut buf)?;

        if &buf[0..5] != MAGIC {
            return Err(err("bad magic: not a .xbin file"));
        }
        let footer_magic = u32::from_le_bytes(buf[80..84].try_into().unwrap());
        if footer_magic != FOOTER_MAGIC {
            return Err(err("bad footer sentinel"));
        }
        let format_version = buf[5];
        if format_version > FORMAT_VERSION {
            return Err(err("unsupported .xbin format version (binary newer than launcher)"));
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
        })
    }

    pub fn sha256_hex(&self) -> String {
        let mut s = String::with_capacity(64);
        for b in &self.payload_sha256 {
            s.push_str(&format!("{:02x}", b));
        }
        s
    }
}

fn err(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}
