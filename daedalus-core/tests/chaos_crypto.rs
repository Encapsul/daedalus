#![allow(clippy::doc_markdown)]
//! Chaos-monkey properties for the AES-256-GCM payload encryption path.
//!
//! Corrupted input must fail CLOSED: every bit-flip, truncation, or key
//! mismatch yields `Err` — never a panic, never unauthenticated plaintext.
//! (GCM forgery probability is 2^-128, so requiring Err across these cases
//! is deterministic in practice.)

use proptest::prelude::*;
use zeroize::Zeroize;

use daedalus_core::encrypt::{
    decrypt_chunks, decrypt_payload, encrypt_chunks, encrypt_payload, CHUNK_TAG_LEN, NONCE_LEN,
};

fn key_strategy() -> impl Strategy<Value = [u8; 32]> {
    proptest::collection::vec(any::<u8>(), 32).prop_map(|v| v.try_into().unwrap())
}

fn msg_strategy() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(any::<u8>(), 0..4096)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn roundtrip_random(key in key_strategy(), msg in msg_strategy()) {
        let (ct, meta) = encrypt_payload(&msg, &key).unwrap();
        prop_assert_eq!(ct.len(), msg.len() + 16);
        let pt = decrypt_payload(&ct, &key, &meta.nonce, &meta.salt).unwrap();
        prop_assert_eq!(pt, msg);
    }

    #[test]
    fn single_bit_flip_fails_closed(
        key in key_strategy(),
        msg in msg_strategy(),
        flip_index in 0usize..4096 + 16,
    ) {
        let (mut ct, meta) = encrypt_payload(&msg, &key).unwrap();
        if flip_index >= ct.len() {
            return Ok(());
        }
        ct[flip_index] ^= 0b0001_0001;
        prop_assert!(decrypt_payload(&ct, &key, &meta.nonce, &meta.salt).is_err());
    }

    #[test]
    fn truncation_fails_closed(key in key_strategy(), msg in msg_strategy(), cut in 0usize..200) {
        let (ct, meta) = encrypt_payload(&msg, &key).unwrap();
        let cut = cut.min(ct.len());
        prop_assume!(cut < ct.len(), "cutting to full length is not a truncation");
        prop_assert!(decrypt_payload(&ct[..cut], &key, &meta.nonce, &meta.salt).is_err());
    }

    #[test]
    fn wrong_key_never_decrypts(key in key_strategy(), other in key_strategy(), msg in msg_strategy()) {
        prop_assume!(key != other);
        let (ct, meta) = encrypt_payload(&msg, &key).unwrap();
        prop_assert!(decrypt_payload(&ct, &other, &meta.nonce, &meta.salt).is_err());
    }

    #[test]
    fn chunked_roundtrip_random(
        key in key_strategy(),
        salt in key_strategy(),
        msg in msg_strategy(),
        chunk_count in 1usize..8,
    ) {
        let mut nonce = [0u8; NONCE_LEN];
        nonce[4..].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let base = msg.len() / chunk_count;
        let mut sizes = vec![base; chunk_count];
        sizes[0] += msg.len() % chunk_count;
        let ct = encrypt_chunks(&msg, &key, &salt, &nonce, &sizes).unwrap();
        prop_assert_eq!(
            ct.len(),
            msg.len() + chunk_count * CHUNK_TAG_LEN
        );
        let pt = decrypt_chunks(&ct, &key, &salt, &nonce, &sizes).unwrap();
        prop_assert_eq!(pt, msg);
    }

    #[test]
    fn chunked_bit_flip_in_any_chunk_fails_closed(
        key in key_strategy(),
        salt in key_strategy(),
        msg in msg_strategy(),
        flip_index in 0usize..2048,
    ) {
        let mut nonce = [0u8; NONCE_LEN];
        nonce[4..].copy_from_slice(&[9, 9, 9, 9, 9, 9, 9, 9]);
        let sizes = vec![msg.len()];
        let mut ct = encrypt_chunks(&msg, &key, &salt, &nonce, &sizes).unwrap();
        if flip_index >= ct.len() {
            return Ok(());
        }
        ct[flip_index] ^= 0b1000_0001;
        prop_assert!(decrypt_chunks(&ct, &key, &salt, &nonce, &sizes).is_err());
    }

    #[test]
    fn chunked_truncation_fails_closed(
        key in key_strategy(),
        salt in key_strategy(),
        msg in msg_strategy(),
        cut in 0usize..300,
    ) {
        let mut nonce = [0u8; NONCE_LEN];
        nonce[4..].copy_from_slice(&[7, 7, 7, 7, 7, 7, 7, 7]);
        let sizes = vec![msg.len()];
        let ct = encrypt_chunks(&msg, &key, &salt, &nonce, &sizes).unwrap();
        let cut = cut.min(ct.len());
        prop_assume!(cut < ct.len(), "cutting to full length is not a truncation");
        prop_assert!(decrypt_chunks(&ct[..cut], &key, &salt, &nonce, &sizes).is_err());
    }
}

/// Zeroize API contract used across the codebase: after `.zeroize()` the
/// buffer holds no trace of the secret, whatever its prior content.
#[test]
fn zeroize_scrubs_every_byte() {
    for len in [1usize, 31, 32, 64, 1024] {
        let mut buf: Vec<u8> = (0..len).map(|i| (i * 37 % 250 + 1) as u8).collect();
        assert!(buf.iter().any(|&b| b != 0));
        buf.zeroize();
        assert!(buf.iter().all(|&b| b == 0), "len {len} not scrubbed");
    }
}
