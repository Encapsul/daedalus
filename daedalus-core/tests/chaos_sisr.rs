//! Chaos: SISR footer extension must fail closed on truncation / bit flips.

use daedalus_core::sisr_header::{SisrFooterExt, SISR_VERSION, SIZE};

fn sample_ext() -> SisrFooterExt {
    SisrFooterExt {
        sisr_version: SISR_VERSION,
        chunk_table_offset: 4096,
        chunk_table_len: 128,
        merkle_root: [0x11; 32],
        signature: [0x22; 64],
    }
}

#[test]
fn chaos_sisr_truncated_pack_rejected() {
    let packed = sample_ext().pack();
    assert!(SisrFooterExt::parse(&packed[..SIZE - 1]).is_err());
    assert!(SisrFooterExt::parse(&[]).is_err());
}

#[test]
fn chaos_sisr_absent_version_is_none_on_read() {
    use std::io::Cursor;

    let mut ext = sample_ext();
    ext.sisr_version = 0;
    let packed = ext.pack();
    // Minimal file: extension + 84-byte stub footer region (zeros).
    let mut file = packed.to_vec();
    file.extend_from_slice(&[0u8; 84]);
    let file_len = file.len() as u64;
    let mut cur = Cursor::new(file);
    let got = SisrFooterExt::read_from(&mut cur, file_len, 84).unwrap();
    assert!(got.is_none(), "sisr_version=0 must read as absent");
}

#[test]
fn chaos_sisr_bitflip_in_version_still_parses_or_absent() {
    // Parse itself is structural; flipping the version byte must not panic
    // and must either yield a present block or absent (version 0).
    let mut packed = sample_ext().pack();
    packed[0] ^= 0xFF;
    let parsed = SisrFooterExt::parse(&packed).expect("parse must not fail on length-ok input");
    let _ = parsed.is_present();
}
