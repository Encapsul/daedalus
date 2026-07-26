//! Zstd compression and decompression for .xbin payload layers.
//!
//! Default compression: level 3 with all CPU cores — BLAZING FAST.
//! Level 19 is ~10x slower for only ~5% smaller output. Use level 19
//! only when binary size is critical and build time doesn't matter.

use std::io::{self, Read, Write};

/// Default compression level: fast, multithreaded, good ratio.
/// Level 3 = fast, 19 = best compression.
pub const DEFAULT_LEVEL: i32 = 3;

/// Compress bytes with zstd (level 3, multi-threaded, BLAZING FAST).
pub fn compress(data: &[u8]) -> io::Result<Vec<u8>> {
    compress_with_level(data, DEFAULT_LEVEL)
}

/// Compress bytes with zstd at a specific level.
/// Level 3 = fast, 19 = best compression. Default for x.bin layers is 3.
pub fn compress_with_level(data: &[u8], level: i32) -> io::Result<Vec<u8>> {
    let mut encoder = zstd::Encoder::new(Vec::new(), level)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    encoder
        .set_pledged_src_size(Some(data.len() as u64))
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    // Use all available CPU cores for parallel compression
    let _ = encoder.multithread(num_cpus());
    encoder
        .write_all(data)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    encoder
        .finish()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}

/// Number of CPU cores available (returns 1 if detection fails).
fn num_cpus() -> u32 {
    std::thread::available_parallelism().map_or(1, |n| n.get() as u32)
}

/// Decompress zstd-compressed bytes.
pub fn decompress(data: &[u8]) -> io::Result<Vec<u8>> {
    let mut decoder =
        zstd::Decoder::new(data).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let mut output = Vec::new();
    decoder
        .read_to_end(&mut output)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let original = b"hello xbin compression test data ".repeat(100);
        let compressed = compress(&original).unwrap();
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(original.as_slice(), decompressed.as_slice());
        assert!(compressed.len() < original.len());
    }

    #[test]
    fn roundtrip_with_level() {
        let original = b"level test data ".repeat(200);
        let fast = compress_with_level(&original, 3).unwrap();
        let best = compress_with_level(&original, 19).unwrap();
        let decompressed_fast = decompress(&fast).unwrap();
        let decompressed_best = decompress(&best).unwrap();
        assert_eq!(original.as_slice(), decompressed_fast.as_slice());
        assert_eq!(original.as_slice(), decompressed_best.as_slice());
        // Higher level = smaller output
        assert!(best.len() <= fast.len());
    }

    #[test]
    fn compress_produces_smaller_output() {
        let data = b"aaaaaaaaaaaa".repeat(1000);
        let compressed = compress(&data).unwrap();
        assert!(compressed.len() < data.len());
    }
}
