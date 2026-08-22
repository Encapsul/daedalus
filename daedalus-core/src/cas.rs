//! Content-addressable object storage (CAS).
//!
//! Stores byte blobs under their own SHA-256 content hash. Writing and
//! reading re-verify the hash, so bit rot or tampering surfaces as an error
//! instead of silently corrupting a reconstruction.
//!
//! STATUS: 2026-08-21 — wired into `registry.rs` (`LayerRegistry`) and
//! `engine.rs` (`DirectoryChunkFetcher` uses `DiskObjectStore` internally).
//! Tests keep it rot-free; production use goes through the `LayerRegistry` API.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// A content-addressable store keyed by SHA-256.
///
/// `put` accepts the caller-supplied hash only if it matches the data; `get`
/// verifies the stored bytes against the requested hash on every read.
pub trait ObjectStore {
    /// Stores `data` under `hash`, verifying `SHA-256(data) == hash`.
    fn put(&mut self, hash: &[u8; 32], data: &[u8]) -> io::Result<()>;

    /// Returns the object for `hash`, or `None` if absent.
    ///
    /// Recomputes `SHA-256` of the stored bytes and errors on mismatch
    /// (bit rot / tampering).
    fn get(&self, hash: &[u8; 32]) -> io::Result<Option<Vec<u8>>>;

    /// Downcast support for introspection (e.g. `DiskObjectStore.root()`).
    fn as_any(&self) -> &dyn std::any::Any;
}

/// In-memory object store — a mock/standalone store for tests and tools.
#[derive(Default)]
pub struct MemoryStore {
    objects: HashMap<[u8; 32], Vec<u8>>,
}

impl MemoryStore {
    /// Creates an empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl ObjectStore for MemoryStore {
    fn put(&mut self, hash: &[u8; 32], data: &[u8]) -> io::Result<()> {
        verify_hash(hash, data)?;
        self.objects.insert(*hash, data.to_vec());
        Ok(())
    }

    fn get(&self, hash: &[u8; 32]) -> io::Result<Option<Vec<u8>>> {
        match self.objects.get(hash) {
            None => Ok(None),
            Some(data) => {
                verify_hash(hash, data)?;
                Ok(Some(data.clone()))
            }
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Filesystem-backed object store rooted at a directory.
///
/// Objects are stored as one file per content hash (hex-encoded name). A
/// collision is impossible unless SHA-256 collides; a name that exists but
/// holds different bytes is detected by the read-time hash check.
pub struct DiskObjectStore {
    root: PathBuf,
}

impl DiskObjectStore {
    /// Creates the store root directory (and parents) if needed.
    pub fn new(root: &Path) -> io::Result<Self> {
        fs::create_dir_all(root)?;
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Returns the on-disk root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path_for(&self, hash: &[u8; 32]) -> PathBuf {
        self.root.join(hex::encode(hash))
    }
}

impl ObjectStore for DiskObjectStore {
    fn put(&mut self, hash: &[u8; 32], data: &[u8]) -> io::Result<()> {
        verify_hash(hash, data)?;
        let path = self.path_for(hash);
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, data)?;
        fs::rename(&tmp, &path)?;
        // Read-back verification catches partial writes / bit rot at write time.
        let written = fs::read(&path)?;
        verify_hash(hash, &written)
    }

    fn get(&self, hash: &[u8; 32]) -> io::Result<Option<Vec<u8>>> {
        let path = self.path_for(hash);
        let data = match fs::read(&path) {
            Ok(data) => data,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        verify_hash(hash, &data)?;
        Ok(Some(data))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Fails unless `SHA-256(data) == expected`.
fn verify_hash(expected: &[u8; 32], data: &[u8]) -> io::Result<()> {
    let actual = Sha256::digest(data);
    if actual.as_slice() != expected.as_slice() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "content hash mismatch (bit rot or tampering)",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash_of(data: &[u8]) -> [u8; 32] {
        Sha256::digest(data).into()
    }

    #[test]
    fn memory_roundtrip() {
        let mut store = MemoryStore::new();
        let data = b"payload block";
        let hash = hash_of(data);
        store.put(&hash, data).unwrap();
        assert_eq!(store.get(&hash).unwrap(), Some(data.to_vec()));
    }

    #[test]
    fn memory_missing_returns_none() {
        let store = MemoryStore::new();
        let absent = [0u8; 32];
        assert_eq!(store.get(&absent).unwrap(), None);
    }

    #[test]
    fn memory_rejects_wrong_hash_on_put() {
        let mut store = MemoryStore::new();
        let wrong = [0u8; 32];
        assert!(store.put(&wrong, b"data").is_err());
        assert!(store.get(&wrong).unwrap().is_none());
    }

    #[test]
    fn disk_roundtrip_and_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = DiskObjectStore::new(dir.path()).unwrap();
        let data = b"squashfs block";
        let hash = hash_of(data);
        store.put(&hash, data).unwrap();

        // A fresh store over the same root must still see the object.
        let reopened = DiskObjectStore::new(dir.path()).unwrap();
        assert_eq!(reopened.get(&hash).unwrap(), Some(data.to_vec()));
    }

    #[test]
    fn disk_detects_tampering_on_read() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = DiskObjectStore::new(dir.path()).unwrap();
        let data = b"content";
        let hash = hash_of(data);
        store.put(&hash, data).unwrap();

        // Corrupt the stored file behind the store's back.
        let path = dir.path().join(hex::encode(hash));
        fs::write(path, b"tampered").unwrap();
        assert!(store.get(&hash).is_err());
    }

    #[test]
    fn disk_detects_wrong_hash_on_put() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = DiskObjectStore::new(dir.path()).unwrap();
        let wrong = [0u8; 32];
        assert!(store.put(&wrong, b"data").is_err());
    }
}
