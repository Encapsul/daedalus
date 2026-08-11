//! Binary delta manifest — the `SISR` chunk index embedded in a `.xbin`.
//!
//! Layout is little-endian with no padding:
//!
//! ```text
//! magic        b"XBMD"              (4 bytes)
//! version      u8                   (1)
//! reserved     [u8; 3]              (zero)
//! chunk_count  u32
//! payload_len  u64
//! chunks       [ChunkEntry; chunk_count]
//! ChunkEntry   hash [u8; 32] + length u32   (36 bytes each)
//! ```
//!
//! Parsing is strict and allocation-safe: the buffer length must equal the
//! length implied by `chunk_count` exactly, and every count-derived size is
//! computed with checked arithmetic before a single allocation.

use std::io;

pub const MAGIC: &[u8; 4] = b"XBMD";
pub const VERSION: u8 = 1;
pub const HEADER_SIZE: usize = 20;
pub const ENTRY_SIZE: usize = 36;

/// One content-addressed chunk of the reconstructed payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkEntry {
    /// `SHA-256` of the chunk bytes — the content address.
    pub hash: [u8; 32],
    /// Byte length of the chunk.
    pub length: u32,
}

/// The embedded `SISR` delta manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeltaManifest {
    /// Manifest schema version (see [`VERSION`]).
    pub version: u8,
    /// Total size in bytes of the reconstructed payload.
    pub payload_len: u64,
    /// Content-addressed chunks, in payload order.
    pub chunks: Vec<ChunkEntry>,
}

impl DeltaManifest {
    /// Byte length of the serialized form.
    pub fn encoded_len(&self) -> usize {
        HEADER_SIZE + self.chunks.len() * ENTRY_SIZE
    }

    /// Serializes the manifest into its compact binary form.
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.encoded_len());
        out.extend_from_slice(MAGIC);
        out.push(VERSION);
        out.extend_from_slice(&[0u8; 3]);
        out.extend_from_slice(&(self.chunks.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.payload_len.to_le_bytes());
        for chunk in &self.chunks {
            out.extend_from_slice(&chunk.hash);
            out.extend_from_slice(&chunk.length.to_le_bytes());
        }
        out
    }

    /// Parses a manifest from a complete serialized buffer.
    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() < HEADER_SIZE {
            return Err(err("truncated delta manifest header"));
        }
        if &bytes[0..4] != MAGIC {
            return Err(err("bad delta manifest magic"));
        }
        let version = bytes[4];
        if version > VERSION {
            return Err(err("unsupported delta manifest version"));
        }
        let chunk_count = u32::from_le_bytes(fixed(&bytes[8..12])?);
        let payload_len = u64::from_le_bytes(fixed(&bytes[12..20])?);
        let table_len = usize::try_from(chunk_count)
            .ok()
            .and_then(|n| n.checked_mul(ENTRY_SIZE))
            .and_then(|n| n.checked_add(HEADER_SIZE))
            .ok_or_else(|| err("delta manifest chunk table too large"))?;
        if bytes.len() != table_len {
            return Err(err("delta manifest length mismatch"));
        }
        let mut chunks = Vec::with_capacity(chunk_count as usize);
        for i in 0..chunk_count as usize {
            let base = HEADER_SIZE + i * ENTRY_SIZE;
            let entry = &bytes[base..base + ENTRY_SIZE];
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&entry[0..32]);
            chunks.push(ChunkEntry {
                hash,
                length: u32::from_le_bytes(fixed(&entry[32..36])?),
            });
        }
        Ok(Self {
            version,
            payload_len,
            chunks,
        })
    }
}

fn fixed<const N: usize>(b: &[u8]) -> io::Result<[u8; N]> {
    b.try_into()
        .map_err(|_| err("truncated delta manifest field"))
}

fn err(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> DeltaManifest {
        DeltaManifest {
            version: VERSION,
            payload_len: 4096,
            chunks: vec![
                ChunkEntry {
                    hash: [1; 32],
                    length: 2048,
                },
                ChunkEntry {
                    hash: [2; 32],
                    length: 2048,
                },
            ],
        }
    }

    #[test]
    fn constants_match_layout() {
        assert_eq!(MAGIC, b"XBMD");
        assert_eq!(ENTRY_SIZE, 32 + 4);
    }

    #[test]
    fn serialize_parse_roundtrip_is_bit_exact() {
        let m = sample();
        let parsed = DeltaManifest::parse(&m.serialize()).unwrap();
        assert_eq!(parsed, m);
        assert_eq!(parsed.version, VERSION);
        assert_eq!(parsed.payload_len, 4096);
        assert_eq!(parsed.chunks[0].hash, [1; 32]);
        assert_eq!(parsed.chunks[1].length, 2048);
    }

    #[test]
    fn empty_manifest_roundtrips() {
        let m = DeltaManifest {
            version: VERSION,
            payload_len: 0,
            chunks: vec![],
        };
        let bytes = m.serialize();
        assert_eq!(bytes.len(), HEADER_SIZE);
        assert_eq!(DeltaManifest::parse(&bytes).unwrap(), m);
    }

    #[test]
    fn encoded_len_matches_serialized_len() {
        let m = sample();
        assert_eq!(m.encoded_len(), m.serialize().len());
    }

    #[test]
    fn parse_rejects_truncated_buffer() {
        let bytes = sample().serialize();
        assert!(DeltaManifest::parse(&bytes[..bytes.len() - 1]).is_err());
        assert!(DeltaManifest::parse(&bytes[..HEADER_SIZE - 1]).is_err());
        assert!(DeltaManifest::parse(&[]).is_err());
    }

    #[test]
    fn parse_rejects_bad_magic() {
        let mut bytes = sample().serialize();
        bytes[0..4].copy_from_slice(b"XXXX");
        assert!(DeltaManifest::parse(&bytes).is_err());
    }

    #[test]
    fn parse_rejects_unsupported_version() {
        let mut bytes = sample().serialize();
        bytes[4] = VERSION + 1;
        assert!(DeltaManifest::parse(&bytes).is_err());
    }

    #[test]
    fn parse_rejects_trailing_garbage() {
        let mut bytes = sample().serialize();
        bytes.push(0x00);
        assert!(DeltaManifest::parse(&bytes).is_err());
    }

    #[test]
    fn parse_rejects_huge_chunk_count_without_allocating() {
        let mut bytes = vec![0u8; HEADER_SIZE];
        bytes[0..4].copy_from_slice(MAGIC);
        bytes[4] = VERSION;
        bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(DeltaManifest::parse(&bytes).is_err());
    }

    #[test]
    fn parse_rejects_chunk_count_larger_than_buffer() {
        let mut bytes = vec![0u8; HEADER_SIZE + ENTRY_SIZE];
        bytes[0..4].copy_from_slice(MAGIC);
        bytes[4] = VERSION;
        bytes[8..12].copy_from_slice(&2u32.to_le_bytes());
        assert!(DeltaManifest::parse(&bytes).is_err());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn chunk() -> impl Strategy<Value = ChunkEntry> {
        (prop::array::uniform32(any::<u8>()), 1u32..=32 << 20)
            .prop_map(|(hash, length)| ChunkEntry { hash, length })
    }

    proptest! {
        #[test]
        fn arbitrary_bytes_never_panic(
            buf in prop::collection::vec(any::<u8>(), 0..4096),
        ) {
            let _ = DeltaManifest::parse(&buf);
        }

        #[test]
        fn serialize_roundtrips(
            payload_len in any::<u64>(),
            chunks in prop::collection::vec(chunk(), 0..64),
        ) {
            let m = DeltaManifest {
                version: VERSION,
                payload_len,
                chunks,
            };
            let parsed = DeltaManifest::parse(&m.serialize()).unwrap();
            prop_assert_eq!(&parsed, &m);
        }
    }
}
