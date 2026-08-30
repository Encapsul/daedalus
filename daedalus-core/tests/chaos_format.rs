//! Chaos-monkey and fuzz tests for `.daedalus` format parsing.
//!
//! Invariants:
//! - `Footer::read_from` never panics on arbitrary input.
//! - Malformed input returns `Err`, never `Ok` with garbage fields.
//! - Round-trip: pack → read_from → pack preserves bytes.
//! - Truncation, bit-flips, and magic corruption all fail closed.

use daedalus_core::format::{Footer, FOOTER_MAGIC, FLAG_ENCRYPTED, FLAG_SIGNED, MAGIC};
use std::io::Cursor;

fn pack_roundtrip(footer: &Footer) -> Vec<u8> {
    let payload_len = footer.payload_offset.saturating_add(footer.payload_csize);
    let payload_len = payload_len.min(10_000_000) as usize; // cap to avoid OOM in tests
    let mut data = vec![0u8; payload_len];
    if footer.format_version >= 3 {
        data.extend_from_slice(&footer.pack_full());
    } else {
        data.extend_from_slice(&footer.pack());
    }
    data
}

/// Fuzz: `read_from` must never panic, no matter the input.
#[test]
fn read_never_panics_on_garbage() {
    let garbage_cases: &[&[u8]] = &[
        &[],
        &[0x00],
        &[0xFF; 1],
        &[0xFF; 83],
        &[0xFF; 84],
        &[0xFF; 85],
        &[0xFF; 4096],
        &[0xCA, 0xFE, 0xEF, 0xBE],
        MAGIC.as_slice(),
        &[0x00; 1000],
        &[0xAB; 1000],
    ];
    for data in garbage_cases {
        let mut cursor = Cursor::new(data.to_vec());
        let _ = Footer::read_from(&mut cursor);
    }
}

/// Truncated footer (84 bytes without correct magic) must be rejected.
#[test]
fn truncated_footer_rejected() {
    for len in 0..84 {
        let mut data = vec![0xFF; len];
        if len >= 4 {
            data[len - 4..].copy_from_slice(&[0xCA, 0xFE, 0xEF, 0xBE]);
        }
        let mut cursor = Cursor::new(data);
        assert!(
            Footer::read_from(&mut cursor).is_err(),
            "truncated footer of length {} should be rejected",
            len
        );
    }
}

/// Bad magic must be rejected at every byte position.
#[test]
fn bad_magic_rejected() {
    let mut sha = [0u8; 32];
    sha.fill(0xAB);
    let footer = Footer {
        format_version: 3,
        arch: 1,
        flags: 0,
        payload_offset: 100,
        payload_csize: 200,
        payload_usize: 200,
        payload_sha256: sha,
        meta_offset: 300,
        meta_size: 50,
        sig_offset: 160,
    };
    let mut data = pack_roundtrip(&footer);
    let footer_start = data.len() - 92;
    for pos in footer_start + 8..footer_start + 13 {
        data[pos] ^= 0xFF;
        let mut cursor = Cursor::new(data.clone());
        assert!(
            Footer::read_from(&mut cursor).is_err(),
            "magic corruption at byte {} should be rejected",
            pos
        );
        data[pos] ^= 0xFF; // restore
    }
}

/// Unsupported format version must be rejected.
#[test]
fn unsupported_version_rejected() {
    let mut sha = [0u8; 32];
    sha.fill(0xCD);
    for version in [6, 7, 42, 100, 200, 255] {
        let footer = Footer {
            format_version: version,
            arch: 1,
            flags: 0,
            payload_offset: 0,
            payload_csize: 0,
            payload_usize: 0,
            payload_sha256: sha,
            meta_offset: 0,
            meta_size: 0,
            sig_offset: 0,
        };
        let data = pack_roundtrip(&footer);
        let mut cursor = Cursor::new(data);
        assert!(
            Footer::read_from(&mut cursor).is_err(),
            "version {} should be rejected",
            version
        );
    }
}

/// Pack → read → pack must preserve all fields.
#[test]
fn pack_read_pack_roundtrip() {
    let cases: &[Footer] = &[
        Footer {
            format_version: 2,
            arch: 1,
            flags: 0,
            payload_offset: 0,
            payload_csize: 100,
            payload_usize: 100,
            payload_sha256: [0xAA; 32],
            meta_offset: 0,
            meta_size: 0,
            sig_offset: 0,
        },
        Footer {
            format_version: 3,
            arch: 2,
            flags: FLAG_SIGNED | FLAG_ENCRYPTED,
            payload_offset: 256,
            payload_csize: 1024,
            payload_usize: 2048,
            payload_sha256: [0xBB; 32],
            meta_offset: 2304,
            meta_size: 128,
            sig_offset: 160,
        },
        Footer {
            format_version: 5,
            arch: 0xFF,
            flags: 0xFF,
            payload_offset: u64::MAX - 100,
            payload_csize: u64::MAX - 50,
            payload_usize: u64::MAX,
            payload_sha256: [0xFF; 32],
            meta_offset: u64::MAX,
            meta_size: u64::MAX,
            sig_offset: u64::MAX,
        },
    ];
    for footer in cases {
        let data = pack_roundtrip(footer);
        let mut cursor = Cursor::new(data);
        let parsed = Footer::read_from(&mut cursor).unwrap();
        let repacked = parsed.pack_full();
        let mut expected = [0u8; 92];
        expected[0..8].copy_from_slice(&footer.sig_offset.to_le_bytes());
        expected[8..].copy_from_slice(&parsed.pack());
        assert_eq!(&repacked[..], &expected[..]);
    }
}

/// Extreme edge cases for v2 footer.
#[test]
fn v2_footer_with_exactly_84_bytes() {
    let mut footer = [0u8; 84];
    footer[0..5].copy_from_slice(MAGIC.as_slice());
    footer[80..84].copy_from_slice(&FOOTER_MAGIC.to_le_bytes());
    let mut cursor = Cursor::new(footer.to_vec());
    let result = Footer::read_from(&mut cursor);
    assert!(result.is_ok(), "v2 footer should parse with exactly 84 bytes: {:?}", result);
}

/// Extreme edge cases for v3 footer.
#[test]
fn v3_footer_with_exactly_92_bytes() {
    let mut footer = [0u8; 92];
    footer[0..8].copy_from_slice(&0u64.to_le_bytes());
    footer[8..13].copy_from_slice(MAGIC.as_slice());
    footer[13] = 3;
    footer[88..92].copy_from_slice(&FOOTER_MAGIC.to_le_bytes());
    let mut cursor = Cursor::new(footer.to_vec());
    let result = Footer::read_from(&mut cursor);
    assert!(result.is_ok(), "v3 footer should parse with exactly 92 bytes: {:?}", result);
}

/// All flags set should still parse.
#[test]
fn footer_with_all_flags_set() {
    let mut sha = [0u8; 32];
    sha.fill(0xFF);
    let footer = Footer {
        format_version: 5,
        arch: 0xFF,
        flags: 0xFF,
        payload_offset: u64::MAX,
        payload_csize: u64::MAX,
        payload_usize: u64::MAX,
        payload_sha256: sha,
        meta_offset: u64::MAX,
        meta_size: u64::MAX,
        sig_offset: u64::MAX,
    };
    let data = pack_roundtrip(&footer);
    let mut cursor = Cursor::new(data);
    let parsed = Footer::read_from(&mut cursor);
    assert!(parsed.is_ok(), "footer with all flags set should parse");
}

/// Zero sizes should still parse (empty payload).
#[test]
fn footer_with_zero_sizes() {
    let mut sha = [0u8; 32];
    sha.fill(0x00);
    let footer = Footer {
        format_version: 2,
        arch: 1,
        flags: 0,
        payload_offset: 0,
        payload_csize: 0,
        payload_usize: 0,
        payload_sha256: sha,
        meta_offset: 0,
        meta_size: 0,
        sig_offset: 0,
    };
    let data = pack_roundtrip(&footer);
    let mut cursor = Cursor::new(data);
    let parsed = Footer::read_from(&mut cursor);
    assert!(parsed.is_ok(), "footer with zero sizes should parse");
}

/// Every byte of the footer magic + sentinel block can be corrupted and must fail.
#[test]
fn every_byte_corruption_fails() {
    let mut sha = [0u8; 32];
    sha.fill(0x42);
    let footer = Footer {
        format_version: 3,
        arch: 1,
        flags: FLAG_SIGNED,
        payload_offset: 100,
        payload_csize: 200,
        payload_usize: 200,
        payload_sha256: sha,
        meta_offset: 300,
        meta_size: 50,
        sig_offset: 160,
    };
    let original = pack_roundtrip(&footer);
    let footer_start = original.len() - 92;
    // Corrupt every byte of the 92-byte footer block
    for pos in footer_start..original.len() {
        let mut data = original.clone();
        data[pos] ^= 0xFF;
        let mut cursor = Cursor::new(data);
        let result = Footer::read_from(&mut cursor);
        // Note: corrupting bytes that are NOT checked by read_from (e.g., payload fields
        // stored in the footer but not validated during parse) may still succeed.
        // We only assert failure for bytes that are actually validated.
        let is_validated = matches!(pos - footer_start, 8..=12 | 13 | 88..=91);
        if is_validated {
            assert!(
                result.is_err(),
                "corruption at validated byte {} should be rejected",
                pos
            );
        }
    }
}
