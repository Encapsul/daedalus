//! `.xbin` format parser — see docs/src/reference/format.md.
//!
//! The launcher reads itself via /proc/self/exe and parses the footer at end-of-file.
//!
//! Footer versions:
//!   v1/v2 — 84 bytes at EOF-84.
//!   v3    — 92 bytes at EOF-92.  The last 84 bytes are byte-identical to v2,
//!           so a v2 launcher reading EOF-84 sees the correct magic + `format_version`
//!           and reports "unsupported format" cleanly.  A v3 launcher reads 92 bytes
//!           and picks `sig_offset` from the 8-byte prefix.
//!   v4    — 92 bytes at EOF-92 (same physical size as v3).  `payload_usize` is
//!           repurposed as `crypto_suite`: 0x00=none, 0x01=AES-256-GCM.
//!           When `crypto_suite=1`, metadata contains "crypto" with `nonce_hex` and
//!           `signing_seed_hex` for AES-256-GCM decryption.
//!   v5    — 92 bytes at EOF-92 (same physical size as v3/v4).  Footer identical
//!           to v4.  Metadata gains `payload_format` field: "zstd-tar" (default,
//!           backward-compatible) or "squashfs".  Launcher checks `payload_format`
//!           to choose extraction strategy (zstd+tar vs squashfs parse+extract).
//!
//! Layout of the 92-byte v3/v4/v5 footer (little-endian):
//!   [0-7]    `sig_offset` (u64)          offset of [`sig_size:u32le` + `sig:64 bytes`]
//!   [8-12]   magic (5 bytes)           "XBIN\x01"
//!   [13]     `format_version` (u8)       3, 4, or 5
//!   [14]     arch (u8)
//!   [15]     flags (u8)                bit0=signed, bit1=encrypted
//!   [16-23]  `payload_offset` (u64)
//!   [24-31]  `payload_csize` (u64)
//!   [32-39]  `payload_usize` (u64)       v2/v3: unused; v4/v5: `crypto_suite`
//!   [40-71]  `payload_sha256` (32 bytes) SHA-256(payload ‖ metadata)
//!   [72-79]  `meta_offset` (u64)
//!   [80-87]  `meta_size` (u64)
//!   [88-91]  `footer_magic` (u32)        0xBEEFCAFE
//!
//! Integrity hash contract (MUST match across Rust + Python):
//!   integrity = SHA-256(compressed_payload_bytes ‖ `metadata_json_bytes`)
//!   This is stored in `payload_sha256` and verified on every cold start.
//!   The same hash is signed by Ed25519 (v3+): sign(integrity, `private_key`).
//!   Implemented: Rust → `main.rs:verify_sha256()`, `verify_ed25519()`
//!               Python → `build.py:build()`, `sign.py:sign()`

use std::io::{self, Read, Seek, SeekFrom};

pub const MAGIC: &[u8; 5] = b"XBIN\x01";
pub const FOOTER_MAGIC: u32 = 0xBEEF_CAFE;
pub const FORMAT_VERSION: u8 = 5;

pub const V2_FOOTER_SIZE: u64 = 84;
pub const V3_FOOTER_SIZE: u64 = 92;

// Crypto suite IDs (stored in payload_usize when format_version >= 4)
pub const CRYPTO_NONE: u64 = 0x00;
pub const CRYPTO_AES_256_GCM: u64 = 0x01;

// Payload format strings (metadata JSON "payload_format" field, format_version >= 5)
#[allow(dead_code)]
pub const PAYLOAD_FORMAT_ZSTD_TAR: &str = "zstd-tar";
pub const PAYLOAD_FORMAT_SQUASHFS: &str = "squashfs";

/// Fixed footer at the very end of a .xbin file.
///
/// `sig_offset` is meaningful only when `format_version >= 3 && flags & FLAG_SIGNED`.
/// For v1/v2 it is always 0.
///
/// `payload_usize` serves double duty:
///   v2/v3: unused (always 0)
///   v4+:   `crypto_suite` (0=none, 1=AES-256-GCM)
#[derive(Debug)]
pub struct Footer {
    pub format_version: u8,
    #[allow(dead_code)]
    pub arch: u8,
    pub flags: u8,
    pub payload_offset: u64,
    pub payload_csize: u64,
    pub payload_usize: u64,
    pub payload_sha256: [u8; 32],
    pub meta_offset: u64,
    pub meta_size: u64,
    /// v3+: absolute offset of the signature block (`[sig_size:u32le][sig:64 bytes]`).
    pub sig_offset: u64,
}

impl Footer {
    /// Crypto suite ID. Only meaningful when `format_version` >= 4.
    pub fn crypto_suite(&self) -> u64 {
        if self.format_version >= 4 {
            self.payload_usize
        } else {
            CRYPTO_NONE
        }
    }
}

fn u64_le(b: &[u8]) -> u64 {
    u64::from_le_bytes(b.try_into().unwrap())
}

impl Footer {
    /// Read and validate the footer from a seekable file.
    ///
    /// Detection order: v3 footer (92 bytes @ EOF-92) → v2 footer (84 bytes @ EOF-84).
    pub fn read_from<R: Read + Seek>(r: &mut R) -> io::Result<Footer> {
        let total = r.seek(SeekFrom::End(0))?;

        // Try v3/v2 footer (92 bytes).
        if total >= V3_FOOTER_SIZE {
            r.seek(SeekFrom::End(-(V3_FOOTER_SIZE as i64)))?;
            let mut buf = [0u8; V3_FOOTER_SIZE as usize];
            r.read_exact(&mut buf)?;

            if &buf[8..13] == MAGIC && buf[13] >= 3 {
                let sig_offset = u64_le(&buf[0..8]);
                return Self::parse(&buf[8..], V3_FOOTER_SIZE, sig_offset);
            }
        }

        // Fallback: v1/v2 footer (84 bytes).
        if total >= V2_FOOTER_SIZE {
            r.seek(SeekFrom::End(-(V2_FOOTER_SIZE as i64)))?;
            let mut buf = [0u8; V2_FOOTER_SIZE as usize];
            r.read_exact(&mut buf)?;

            if &buf[0..5] == MAGIC {
                return Self::parse(&buf, V2_FOOTER_SIZE, 0);
            }
            return Err(err("bad magic: not a .xbin file"));
        }

        Err(err("file too small to be a .xbin"))
    }

    /// Parse the 84-byte core footer (identical bytes for v1/v2/v3).
    fn parse(buf: &[u8], _size: u64, sig_offset: u64) -> io::Result<Footer> {
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
            sig_offset,
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

pub const FLAG_SIGNED: u8 = 0x01;
#[allow(dead_code)]
pub const FLAG_ENCRYPTED: u8 = 0x02;

/// Read `len` bytes at absolute offset `off`.
pub fn read_at<R: Read + Seek>(f: &mut R, off: u64, len: usize) -> io::Result<Vec<u8>> {
    f.seek(SeekFrom::Start(off))?;
    let mut buf = vec![0u8; len];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

fn err(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}
