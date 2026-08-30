//! Chaos-monkey tests for the chunker (SISR delta-indexing).
//!
//! Invariants:
//! - Chunk boundaries are deterministic for the same input + seed.
//! - Empty input yields empty chunk list.
//! - Single-byte input yields at most one chunk.
//! - Chunk sizes are within [min_chunk, max_chunk].
//! - Total chunked bytes equals original length.

use daedalus_core::chunker::{Chunker, FastCDC};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn empty_input_yields_no_chunks(avg in 4usize..=65536) {
        let chunker = FastCDC::new(avg).unwrap();
        let chunks = chunker.chunk(&[]);
        prop_assert!(chunks.is_empty());
    }

    #[test]
    fn single_byte_yields_at_most_one_chunk(avg in 4usize..=65536) {
        let chunker = FastCDC::new(avg).unwrap();
        let chunks = chunker.chunk(&[0xAB]);
        prop_assert!(chunks.len() <= 1);
    }

    #[test]
    fn chunk_sizes_respect_max(
        data in proptest::collection::vec(any::<u8>(), 1..4096),
        avg in 4usize..4096,
    ) {
        let chunker = FastCDC::new(avg).unwrap();
        let chunks = chunker.chunk(&data);
        let max = avg * 4;
        for chunk in &chunks {
            prop_assert!(
                chunk.length <= max,
                "chunk size {} > max {}",
                chunk.length,
                max
            );
        }
    }

    #[test]
    fn deterministic_for_same_input(avg in 4usize..=65536, data in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let chunker = FastCDC::new(avg).unwrap();
        let first = chunker.chunk(&data);
        let second = chunker.chunk(&data);
        prop_assert_eq!(first, second);
    }

    #[test]
    fn total_chunked_equals_original(avg in 4usize..=65536, data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let chunker = FastCDC::new(avg).unwrap();
        let chunks = chunker.chunk(&data);
        let total: usize = chunks.iter().map(|c| c.length).sum();
        prop_assert_eq!(total, data.len());
    }

    #[test]
    fn chunks_cover_buffer_contiguously(avg in 4usize..=65536, data in proptest::collection::vec(any::<u8>(), 1..4096)) {
        let chunker = FastCDC::new(avg).unwrap();
        let chunks = chunker.chunk(&data);
        let mut pos = 0usize;
        for chunk in &chunks {
            prop_assert_eq!(chunk.offset, pos, "chunk offset mismatch");
            let chunk_bytes = &data[chunk.offset..chunk.offset + chunk.length];
            prop_assert_eq!(chunk_bytes.len(), chunk.length);
            pos += chunk.length;
        }
        prop_assert_eq!(pos, data.len(), "chunks don't cover entire buffer");
    }
}
