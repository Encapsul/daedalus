//! Zstd compression and decompression for .xbin payload layers.

use std::io::{self, Read, Write};

/// Compress bytes with zstd (level 19, multi-threaded, optimized for size).
pub fn compress(data: &[u8]) -> io::Result<Vec<u8>> {
    compress_with_level(data, 19)
}

/// Compress bytes with zstd at a specific level.
/// Level 3 = fast, 19 = best compression. Default for x.bin layers is 19.
pub fn compress_with_level(data: &[u8], level: i32) -> io::Result<Vec<u8>> {
    let mut encoder = zstd::Encoder::new(Vec::new(), level)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    encoder
        .set_pledged_src_size(Some(data.len() as u64))
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    encoder
        .write_all(data)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    encoder
        .finish()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}

/// Compress bytes with zstd using multiple threads (like `zstd -T0`).
pub fn compress_mt(data: &[u8]) -> io::Result<Vec<u8>> {
    compress_mt_with_level(data, 19)
}

/// Compress with multi-threading at a specific level.
pub fn compress_mt_with_level(data: &[u8], level: i32) -> io::Result<Vec<u8>> {
    let mut encoder = zstd::Encoder::new(Vec::new(), level)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    encoder
        .set_pledged_src_size(Some(data.len() as u64))
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    encoder
        .write_all(data)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    encoder
        .finish()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}

/// Decompress zstd-compressed bytes.
pub fn decompress(data: &[u8]) -> io::Result<Vec<u8>> {
    let mut decoder = zstd::Decoder::new(data)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let mut output = Vec::new();
    decoder
        .read_to_end(&mut output)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    Ok(output)
}

/// Compress a tar archive (bytes) with zstd. Returns the compressed payload.
pub fn compress_tar_zstd(tar_bytes: &[u8]) -> io::Result<Vec<u8>> {
    compress(tar_bytes)
}

/// Decompress a zstd payload back to raw bytes.
pub fn decompress_zstd(data: &[u8]) -> io::Result<Vec<u8>> {
    decompress(data)
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
