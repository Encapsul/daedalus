//! Content-defined chunking (CDC).
//!
//! SISR rebuilds a binary from deltas. The primitive that turns a byte stream
//! into stable, reusable pieces is content-defined chunking: boundaries are
//! derived from the content itself, so an edit only invalidates the chunks it
//! touches. Pure in-memory, deterministic, and free of any external tool.

use std::io;
use std::sync::LazyLock;

use sha2::{Digest, Sha256};

/// One content-defined piece of a buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkDescriptor {
    /// Offset of the chunk in the source buffer.
    pub offset: usize,
    /// Length of the chunk in bytes.
    pub length: usize,
    /// SHA-256 of the chunk bytes — the content address.
    pub hash: [u8; 32],
}

/// Splits a byte buffer into content-defined chunks.
///
/// Implementations must be deterministic: identical input must always yield
/// identical boundaries, across runs and platforms.
pub trait Chunker {
    /// Returns the chunks of `data`, covering it entirely, in order.
    fn chunk(&self, data: &[u8]) -> Vec<ChunkDescriptor>;
}

/// `FastCDC` with the two-mask normalization described in the `FastCDC` paper.
///
/// Bound rule: `min = avg/4`, `max = avg*4`. A chunk is cut when the rolling
/// fingerprint satisfies `fp & mask == 0`, using a denser mask below `avg`
/// (cut less eagerly) and a sparser mask above it (cut more eagerly), which
/// keeps chunk sizes close to the average.
pub struct FastCDC {
    min: usize,
    avg: usize,
    max: usize,
}

impl FastCDC {
    /// Default average chunk size (8 KiB).
    pub const DEFAULT_AVG_SIZE: usize = 8192;

    /// Chunker with the classic `FastCDC` bounds around `avg_size`.
    pub fn new(avg_size: usize) -> io::Result<Self> {
        if avg_size < 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "average chunk size must be at least 4 bytes",
            ));
        }
        Self::with_bounds(avg_size / 4, avg_size, avg_size.saturating_mul(4))
    }

    /// Chunker with explicit bounds.
    ///
    /// Returns `InvalidInput` if `min_size == 0` or the bounds are not ordered.
    pub fn with_bounds(min_size: usize, avg_size: usize, max_size: usize) -> io::Result<Self> {
        if min_size == 0 || min_size > avg_size || avg_size > max_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "chunk bounds must satisfy 0 < min <= avg <= max",
            ));
        }
        Ok(Self {
            min: min_size,
            avg: avg_size,
            max: max_size,
        })
    }
}

impl Default for FastCDC {
    fn default() -> Self {
        // 2 KiB / 8 KiB / 32 KiB — valid by construction (min > 0, ordered).
        Self {
            min: Self::DEFAULT_AVG_SIZE / 4,
            avg: Self::DEFAULT_AVG_SIZE,
            max: Self::DEFAULT_AVG_SIZE.saturating_mul(4),
        }
    }
}

impl Chunker for FastCDC {
    fn chunk(&self, data: &[u8]) -> Vec<ChunkDescriptor> {
        let mask_small = (1u64 << self.avg.ilog2()) - 1;
        let mask_large = (1u64 << self.max.ilog2()) - 1;
        let mut chunks = Vec::with_capacity(data.len() / self.avg + 1);
        let mut start = 0;
        while start < data.len() {
            let end = self.next_boundary(data, start, mask_small, mask_large);
            let slice = &data[start..end];
            chunks.push(ChunkDescriptor {
                offset: start,
                length: end - start,
                hash: Sha256::digest(slice).into(),
            });
            start = end;
        }
        chunks
    }
}

impl FastCDC {
    /// First cut position at or after `start + min_size`, capped at `max_size`.
    ///
    /// The fingerprint is a rolling gear hash; a cut is emitted when the
    /// normalized mask matches. The first `min_size` bytes are skipped (a cut
    /// there is impossible by definition of the minimum).
    fn next_boundary(&self, data: &[u8], start: usize, mask_small: u64, mask_large: u64) -> usize {
        let end = data.len();
        if end - start <= self.min {
            return end;
        }
        let hard_max = end.min(start.saturating_add(self.max));
        let mut fp: u64 = 0;
        let mut i = start + self.min;
        while i < hard_max {
            fp = fp.wrapping_shl(1).wrapping_add(GEAR[data[i] as usize]);
            let size = i - start + 1;
            let mask = if size <= self.avg {
                mask_small
            } else {
                mask_large
            };
            if fp & mask == 0 {
                return i + 1;
            }
            i += 1;
        }
        hard_max
    }
}

/// Fixed 256-entry gear table generated from a constant seed (xorshift64).
///
/// A fixed table keeps chunk boundaries deterministic across runs and
/// platforms; it need not be cryptographically random.
static GEAR: LazyLock<[u64; 256]> = LazyLock::new(|| {
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    std::array::from_fn(|_| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    })
});

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random buffer (xorshift) — reproducible across runs.
    fn random_buf(len: usize, seed: u64) -> Vec<u8> {
        let mut state = seed;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect()
    }

    fn chunker() -> FastCDC {
        FastCDC::new(1024).expect("valid average size")
    }

    #[test]
    fn chunks_cover_buffer_exactly() {
        let data = random_buf(100_000, 42);
        let chunks = chunker().chunk(&data);
        assert!(!chunks.is_empty());
        let mut pos = 0;
        for chunk in &chunks {
            assert_eq!(chunk.offset, pos);
            pos += chunk.length;
        }
        assert_eq!(pos, data.len());
    }

    #[test]
    fn chunk_sizes_within_bounds() {
        let c = chunker();
        let chunks = c.chunk(&random_buf(100_000, 7));
        for (i, chunk) in chunks.iter().enumerate() {
            assert!(chunk.length <= c.max, "chunk {} too large", i);
            if i + 1 < chunks.len() {
                assert!(chunk.length >= c.min, "chunk {} too small", i);
            }
        }
    }

    #[test]
    fn deterministic_across_instances_and_runs() {
        let data = random_buf(50_000, 99);
        assert_eq!(chunker().chunk(&data), chunker().chunk(&data));
    }

    #[test]
    fn hashes_match_content() {
        let data = random_buf(30_000, 5);
        for chunk in chunker().chunk(&data) {
            let expect: [u8; 32] =
                Sha256::digest(&data[chunk.offset..chunk.offset + chunk.length]).into();
            assert_eq!(chunk.hash, expect);
        }
    }

    #[test]
    fn edit_is_local_to_touched_chunks() {
        let data = random_buf(80_000, 3);
        let chunks = chunker().chunk(&data);
        let edit = chunks[2].offset;
        let mut edited = data.clone();
        edited.insert(edit, 0xAB);
        let edited_chunks = chunker().chunk(&edited);
        // Chunks fully before the edit point are unchanged.
        let mut i = 0;
        while i < chunks.len() && chunks[i].offset + chunks[i].length <= edit {
            assert_eq!(chunks[i], edited_chunks[i]);
            i += 1;
        }
    }

    #[test]
    fn tiny_input_is_single_chunk() {
        let chunks = chunker().chunk(b"hello");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].length, 5);
    }

    #[test]
    fn empty_input_yields_no_chunks() {
        assert!(chunker().chunk(&[]).is_empty());
    }

    #[test]
    fn invalid_bounds_rejected() {
        assert!(FastCDC::with_bounds(0, 100, 200).is_err());
        assert!(FastCDC::with_bounds(100, 50, 200).is_err());
        assert!(FastCDC::new(0).is_err());
    }
}
