//! Binary delta manifest - the `SISR` chunk index embedded in a `.daedalus`.
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

use sha2::{Digest, Sha256};

pub const MAGIC: &[u8; 4] = b"XBMD";
/// Legacy schema. Kept readable so existing signed SISR binaries still verify.
pub const VERSION: u8 = 1;
/// Current schema: adds the signed identity block below.
pub const CURRENT_VERSION: u8 = 2;
pub const HEADER_SIZE: usize = 20;
/// sequence u64 + five SHA-256 digests.
pub const V2_IDENTITY_SIZE: usize = 8 + 5 * 32;
pub const V2_HEADER_SIZE: usize = HEADER_SIZE + V2_IDENTITY_SIZE;
pub const ENTRY_SIZE: usize = 36;

/// Signed identity of the artifact a manifest belongs to.
///
/// v1 manifests predate this block and are accepted only for at-rest
/// verification of legacy binaries. v2 update manifests must carry it so a
/// valid signature from another app, another launcher, or an older sequence
/// cannot be replayed against this binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestIdentity {
    pub sequence: u64,
    pub app_sha256: [u8; 32],
    pub app_version_sha256: [u8; 32],
    pub stub_sha256: [u8; 32],
    pub meta_sha256: [u8; 32],
    pub payload_sha256: [u8; 32],
}

impl ManifestIdentity {
    /// Derives the identity from the bytes the launcher will actually use.
    pub fn new(
        sequence: u64,
        app: &str,
        app_version: &str,
        stub: &[u8],
        meta: &[u8],
        payload: &[u8],
    ) -> Self {
        Self {
            sequence,
            app_sha256: hash_text(app),
            app_version_sha256: hash_text(app_version),
            stub_sha256: sha256(stub),
            meta_sha256: sha256(meta),
            payload_sha256: sha256(payload),
        }
    }
}

fn hash_text(text: &str) -> [u8; 32] {
    sha256(text.as_bytes())
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn header_size(version: u8) -> usize {
    if version >= CURRENT_VERSION {
        V2_HEADER_SIZE
    } else {
        HEADER_SIZE
    }
}

/// One content-addressed chunk of the reconstructed payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkEntry {
    /// `SHA-256` of the chunk bytes - the content address.
    pub hash: [u8; 32],
    /// Byte length of the chunk.
    pub length: u32,
}

/// The embedded `SISR` delta manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeltaManifest {
    /// Manifest schema version (see [`VERSION`] and [`CURRENT_VERSION`]).
    pub version: u8,
    /// Total size in bytes of the reconstructed payload.
    pub payload_len: u64,
    /// Content-addressed chunks, in payload order.
    pub chunks: Vec<ChunkEntry>,
    /// Signed identity block. `None` only for legacy v1 manifests.
    pub identity: Option<ManifestIdentity>,
}

impl DeltaManifest {
    /// Byte length of the serialized form.
    ///
    /// # Panics
    ///
    /// Panics if the computed length overflows `usize`. This is unreachable
    /// in practice because `chunks.len()` is bounded by the buffer length
    /// validated in `parse()`.
    pub fn encoded_len(&self) -> usize {
        let header = header_size(self.version);
        // SAFETY: `chunks.len()` is bounded by the buffer length validated in
        // parse(). The maximum representable value is
        // (usize::MAX - V2_HEADER_SIZE) / ENTRY_SIZE, far larger than any buffer.
        #[allow(clippy::expect_used)]
        self.chunks
            .len()
            .checked_mul(ENTRY_SIZE)
            .and_then(|n| n.checked_add(header))
            .expect("manifest size overflow")
    }

    /// Serializes the manifest into its compact binary form.
    ///
    /// Returns an error if the chunk count exceeds `u32::MAX`.
    pub fn serialize(&self) -> io::Result<Vec<u8>> {
        if self.version == 0 || self.version > CURRENT_VERSION {
            return Err(err("unsupported delta manifest version"));
        }
        let identity = if self.version >= CURRENT_VERSION {
            Some(
                self.identity
                    .as_ref()
                    .ok_or_else(|| err("v2 delta manifest requires an identity block"))?,
            )
        } else {
            None
        };
        let chunk_count =
            u32::try_from(self.chunks.len()).map_err(|_| err("chunk count exceeds u32::MAX"))?;
        let mut out = Vec::with_capacity(self.encoded_len());
        out.extend_from_slice(MAGIC);
        out.push(self.version);
        out.extend_from_slice(&[0u8; 3]);
        out.extend_from_slice(&chunk_count.to_le_bytes());
        out.extend_from_slice(&self.payload_len.to_le_bytes());
        if let Some(id) = identity {
            out.extend_from_slice(&id.sequence.to_le_bytes());
            out.extend_from_slice(&id.app_sha256);
            out.extend_from_slice(&id.app_version_sha256);
            out.extend_from_slice(&id.stub_sha256);
            out.extend_from_slice(&id.meta_sha256);
            out.extend_from_slice(&id.payload_sha256);
        }
        for chunk in &self.chunks {
            out.extend_from_slice(&chunk.hash);
            out.extend_from_slice(&chunk.length.to_le_bytes());
        }
        Ok(out)
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
        if version == 0 || version > CURRENT_VERSION {
            return Err(err("unsupported delta manifest version"));
        }
        if bytes[5..8] != [0u8; 3] {
            return Err(err("nonzero delta manifest reserved bytes"));
        }
        let header = header_size(version);
        if bytes.len() < header {
            return Err(err("truncated delta manifest identity"));
        }
        let chunk_count = u32::from_le_bytes(fixed(&bytes[8..12])?);
        let payload_len = u64::from_le_bytes(fixed(&bytes[12..20])?);
        let identity = if version >= CURRENT_VERSION {
            let off = HEADER_SIZE;
            Some(ManifestIdentity {
                sequence: u64::from_le_bytes(fixed(&bytes[off..off + 8])?),
                app_sha256: fixed(&bytes[off + 8..off + 40])?,
                app_version_sha256: fixed(&bytes[off + 40..off + 72])?,
                stub_sha256: fixed(&bytes[off + 72..off + 104])?,
                meta_sha256: fixed(&bytes[off + 104..off + 136])?,
                payload_sha256: fixed(&bytes[off + 136..off + 168])?,
            })
        } else {
            None
        };
        let table_len = usize::try_from(chunk_count)
            .ok()
            .and_then(|n| n.checked_mul(ENTRY_SIZE))
            .and_then(|n| n.checked_add(header))
            .ok_or_else(|| err("delta manifest chunk table too large"))?;
        if bytes.len() != table_len {
            return Err(err("delta manifest length mismatch"));
        }
        let mut chunks = Vec::with_capacity(chunk_count as usize);
        for i in 0..chunk_count as usize {
            let base = header + i * ENTRY_SIZE;
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
            identity,
        })
    }
}

fn fixed<const N: usize>(b: &[u8]) -> io::Result<[u8; N]> {
    b.try_into()
        .map_err(|_| err("truncated delta manifest field"))
}

/// `err` - err.
/// `@msg`: message
/// `@io`: io
///
/// Description:
///
/// Return: the `std::io::Error`
fn err(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `sample` - sample.
    ///
    /// Description:
    ///
    /// Return: the `DeltaManifest`
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
            identity: None,
        }
    }

    fn sample_v2() -> DeltaManifest {
        DeltaManifest {
            version: CURRENT_VERSION,
            payload_len: 4096,
            chunks: sample().chunks,
            identity: Some(ManifestIdentity::new(
                7,
                "demo-app",
                "1.2.3",
                b"stub-bytes",
                br#"{"name":"demo-app"}"#,
                &[9u8; 4096],
            )),
        }
    }

    #[test]
    /// `constants_match_layout` - constants match layout.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn constants_match_layout() {
        assert_eq!(MAGIC, b"XBMD");
        assert_eq!(ENTRY_SIZE, 32 + 4);
    }

    #[test]
    /// `serialize_parse_roundtrip_is_bit_exact` - serialize parse roundtrip is bit exact.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn serialize_parse_roundtrip_is_bit_exact() {
        let m = sample();
        let parsed = DeltaManifest::parse(&m.serialize().unwrap()).unwrap();
        assert_eq!(parsed, m);
        assert_eq!(parsed.version, VERSION);
        assert_eq!(parsed.payload_len, 4096);
        assert_eq!(parsed.chunks[0].hash, [1; 32]);
        assert_eq!(parsed.chunks[1].length, 2048);
    }

    #[test]
    /// `empty_manifest_roundtrips` - empty manifest roundtrips.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn empty_manifest_roundtrips() {
        let m = DeltaManifest {
            version: VERSION,
            payload_len: 0,
            chunks: vec![],
            identity: None,
        };
        let bytes = m.serialize().unwrap();
        assert_eq!(bytes.len(), HEADER_SIZE);
        assert_eq!(DeltaManifest::parse(&bytes).unwrap(), m);
    }

    #[test]
    /// `encoded_len_matches_serialized_len` - encoded len matches serialized len.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn encoded_len_matches_serialized_len() {
        let m = sample();
        assert_eq!(m.encoded_len(), m.serialize().unwrap().len());
    }

    #[test]
    /// `parse_rejects_truncated_buffer` - parse rejects truncated buffer.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn parse_rejects_truncated_buffer() {
        let bytes = sample().serialize().unwrap();
        assert!(DeltaManifest::parse(&bytes[..bytes.len() - 1]).is_err());
        assert!(DeltaManifest::parse(&bytes[..HEADER_SIZE - 1]).is_err());
        assert!(DeltaManifest::parse(&[]).is_err());
    }

    #[test]
    /// `parse_rejects_bad_magic` - parse rejects bad magic.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn parse_rejects_bad_magic() {
        let mut bytes = sample().serialize().unwrap();
        bytes[0..4].copy_from_slice(b"XXXX");
        assert!(DeltaManifest::parse(&bytes).is_err());
    }

    #[test]
    /// `parse_rejects_unsupported_version` - parse rejects unsupported version.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn parse_rejects_unsupported_version() {
        let mut bytes = sample().serialize().unwrap();
        bytes[4] = CURRENT_VERSION + 1;
        assert!(DeltaManifest::parse(&bytes).is_err());
        bytes[4] = 0;
        assert!(DeltaManifest::parse(&bytes).is_err());
    }

    #[test]
    fn parse_rejects_nonzero_reserved_bytes() {
        let mut bytes = sample().serialize().unwrap();
        bytes[5] = 1;
        assert!(DeltaManifest::parse(&bytes).is_err());
    }

    #[test]
    fn v2_identity_roundtrips_and_is_signed() {
        let m = sample_v2();
        let bytes = m.serialize().unwrap();
        assert_eq!(bytes.len(), m.encoded_len());
        assert_eq!(DeltaManifest::parse(&bytes).unwrap(), m);
        assert!(bytes[HEADER_SIZE..].starts_with(&7u64.to_le_bytes()));
    }

    #[test]
    fn v2_requires_identity() {
        let mut m = sample_v2();
        m.identity = None;
        assert!(m.serialize().is_err());
        let mut bytes = sample_v2().serialize().unwrap();
        bytes[4] = VERSION;
        assert!(DeltaManifest::parse(&bytes).is_err());
    }

    #[test]
    /// `parse_rejects_trailing_garbage` - parse rejects trailing garbage.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn parse_rejects_trailing_garbage() {
        let mut bytes = sample().serialize().unwrap();
        bytes.push(0x00);
        assert!(DeltaManifest::parse(&bytes).is_err());
    }

    #[test]
    /// `parse_rejects_huge_chunk_count_without_allocating` - parse rejects huge chunk count without allocating.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn parse_rejects_huge_chunk_count_without_allocating() {
        let mut bytes = vec![0u8; HEADER_SIZE];
        bytes[0..4].copy_from_slice(MAGIC);
        bytes[4] = VERSION;
        bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(DeltaManifest::parse(&bytes).is_err());
    }

    #[test]
    /// `parse_rejects_chunk_count_larger_than_buffer` - parse rejects chunk count larger than buffer.
    ///
    /// Description:
    ///
    /// Return: nothing
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

    /// `chunk` - chunk.
    ///
    /// Description:
    ///
    /// Return: the `impl Strategy<Value = ChunkEntry>`
    fn chunk() -> impl Strategy<Value = ChunkEntry> {
        (prop::array::uniform32(any::<u8>()), 1u32..=32 << 20)
            .prop_map(|(hash, length)| ChunkEntry { hash, length })
    }

    proptest! {
        #[test]
        /// `arbitrary_bytes_never_panic` - arbitrary bytes never panic.
        ///
        /// Description:
        ///
        /// Return: nothing
        fn arbitrary_bytes_never_panic(
            buf in prop::collection::vec(any::<u8>(), 0..4096),
        ) {
            let _ = DeltaManifest::parse(&buf);
        }

        #[test]
        /// `serialize_roundtrips` - serialize roundtrips.
        ///
        /// Description:
        ///
        /// Return: nothing
        fn serialize_roundtrips(
            payload_len in any::<u64>(),
            chunks in prop::collection::vec(chunk(), 0..64),
        ) {
            let m = DeltaManifest {
                version: VERSION,
                payload_len,
                chunks,
                identity: None,
            };
            let parsed = DeltaManifest::parse(&m.serialize().unwrap()).unwrap();
            prop_assert_eq!(&parsed, &m);
        }
    }
}
