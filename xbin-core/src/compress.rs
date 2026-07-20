//! Zstd compression and decompression for .xbin payload layers.

use std::io::{self, Read, Write};

/// Compress bytes with zstd (level 19, optimized for size).
pub fn compress(data: &[u8]) -> io::Result<Vec<u8>> {
    let mut encoder = zstd::Encoder::new(Vec::new(), 19)
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
}
