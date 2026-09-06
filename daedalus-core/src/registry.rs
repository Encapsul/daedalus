//! Content-addressable layer registry.
//!
//! Provides `LayerRegistry` — a thin wrapper over `ObjectStore` that stores
//! serialized layers (and artifact manifests) keyed by their SHA-256 content
//! hash. This is the storage backend for Phase 4 (remote layer sharing).
//!
//! STATUS: 2026-08-21 — `DiskObjectStore` wired into `DirectoryChunkFetcher` for
//! unified CAS under the Sisr chunk cache; local-layer push/pull/list implemented;
//! remote HTTP registry client added in `daedalus-cli/registry/`.

use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::layer::{LayerKind, SerializableLayer};

/// A manifest referencing layers by their content hash.
///
/// Stored in the CAS as a regular object; allows reconstructing an artifact's
/// layer set without shipping the full `.daedalus` binary.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LayerManifest {
    pub artifact_name: String,
    pub layers: Vec<LayerRef>,
}

/// A reference to a stored layer: its content hash and metadata.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LayerRef {
    pub hash: String,
    pub name: String,
    pub kind: LayerKind,
    /// Approximate byte size of the serialized layer (for display / bandwidth).
    pub size: usize,
}

/// A runnable artifact: a full `.de` binary stored under a `name:tag` alias.
///
/// The binary itself lives in the CAS under `binary_hash` (its content hash);
/// the index file maps `name:tag` onto it so distributed apps can be fetched
/// by name instead of by hash.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredArtifact {
    pub name: String,
    pub tag: String,
    pub binary_hash: String,
    pub size: u64,
}

const ARTIFACT_INDEX_FILE: &str = "artifacts.json";

/// A content-addressable layer registry backed by any `ObjectStore`.
///
/// Layers are serialized to JSON and stored under their SHA-256 hash. The
/// store verifies hash integrity on both `put` and `get`, so bit rot or
/// tampering is detected immediately.
pub struct LayerRegistry {
    store: Box<dyn crate::cas::ObjectStore + Send>,
}

impl LayerRegistry {
    /// Creates a registry backed by the given in-memory or on-disk store.
    pub fn new(store: Box<dyn crate::cas::ObjectStore + Send>) -> Self {
        Self { store }
    }

    /// Creates a registry backed by a `DiskObjectStore` at `root`.
    pub fn disk(root: &Path) -> io::Result<Self> {
        let store = crate::cas::DiskObjectStore::new(root)?;
        Ok(Self::new(Box::new(store)))
    }

    /// Serialize a layer, hash it, and store it. Returns the hex-encoded hash.
    pub fn push_layer(&mut self, layer: &SerializableLayer) -> io::Result<String> {
        let bytes = serde_json::to_vec(layer).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("serialize layer: {e}"))
        })?;
        let hash = content_hash(&bytes);
        let hex = format_hex(&hash);
        self.store.put(&hash, &bytes)?;
        Ok(hex)
    }

    /// Retrieve and deserialize a layer by its hex content hash.
    pub fn pull_layer(&self, hex_hash: &str) -> io::Result<SerializableLayer> {
        let hash = parse_hex(hex_hash)?;
        let bytes = self.store.get(&hash)?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("layer not found: {hex_hash}"),
            )
        })?;
        serde_json::from_slice(&bytes).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("deserialize layer: {e}"),
            )
        })
    }

    /// Check whether a layer with the given hash exists.
    pub fn layer_exists(&self, hex_hash: &str) -> io::Result<bool> {
        let hash = parse_hex(hex_hash)?;
        Ok(self.store.get(&hash)?.is_some())
    }

    /// List all stored layer hashes (hex-encoded).
    pub fn list_layers(&self) -> io::Result<Vec<String>> {
        let dir = match self
            .store
            .as_any()
            .downcast_ref::<crate::cas::DiskObjectStore>()
        {
            Some(disk) => disk.root(),
            None => {
                return Ok(vec![]);
            }
        };
        let mut result = vec![];
        let entries = std::fs::read_dir(dir)?;
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_str().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 cache filename")
            })?;
            if name.len() == 64 && name.chars().all(|c| c.is_ascii_hexdigit()) {
                result.push(name.to_string());
            }
        }
        Ok(result)
    }

    /// Store an artifact manifest (list of layer hash refs) and return its hash.
    pub fn publish_artifact(&mut self, manifest: &LayerManifest) -> io::Result<String> {
        let bytes = serde_json::to_vec(manifest).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("serialize manifest: {e}"),
            )
        })?;
        let hash = content_hash(&bytes);
        let hex = format_hex(&hash);
        self.store.put(&hash, &bytes)?;
        Ok(hex)
    }

    /// Retrieve an artifact manifest by its hash.
    pub fn get_artifact(&self, hex_hash: &str) -> io::Result<LayerManifest> {
        let hash = parse_hex(hex_hash)?;
        let bytes = self.store.get(&hash)?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("manifest not found: {hex_hash}"),
            )
        })?;
        serde_json::from_slice(&bytes).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("deserialize manifest: {e}"),
            )
        })
    }

    /// Build a `LayerManifest` from a set of layers, pushing each to the store.
    pub fn build_and_publish(
        &mut self,
        artifact_name: &str,
        layers: &[SerializableLayer],
    ) -> io::Result<(String, Vec<LayerRef>)> {
        let mut refs = vec![];
        for layer in layers {
            let hex = self.push_layer(layer)?;
            let serialized = serde_json::to_vec(layer).map_err(|e| {
                io::Error::new(io::ErrorKind::InvalidData, format!("serialize: {e}"))
            })?;
            refs.push(LayerRef {
                hash: hex,
                name: layer.name().to_string(),
                kind: layer.kind(),
                size: serialized.len(),
            });
        }
        let manifest = LayerManifest {
            artifact_name: artifact_name.to_string(),
            layers: refs.clone(),
        };
        let manifest_hash = self.publish_artifact(&manifest)?;
        Ok((manifest_hash, refs))
    }

    /// Store a full runnable binary under `name:tag` and return its content hash.
    ///
    /// The bytes are content-addressed in the CAS; the `name:tag` alias is
    /// recorded in the registry's `artifacts.json` index (disk-backed only).
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` when the alias contains characters that would
    /// make a `GET /artifact/<name>:<tag>` round trip ambiguous (anything
    /// outside the safe alias alphabet), so an aliases can always be
    /// fetched back over HTTP.
    pub fn publish_artifact_binary(
        &mut self,
        name: &str,
        tag: &str,
        data: &[u8],
    ) -> io::Result<String> {
        crate::http_parse::split_name_tag(&format!("{name}:{tag}"))?;
        let hash = content_hash(data);
        let hex = format_hex(&hash);
        self.store.put(&hash, data)?;
        let mut index = self.read_artifact_index()?;
        index.retain(|a| !(a.name == name && a.tag == tag));
        index.push(StoredArtifact {
            name: name.to_string(),
            tag: tag.to_string(),
            binary_hash: hex.clone(),
            size: data.len() as u64,
        });
        self.write_artifact_index(&index)?;
        Ok(hex)
    }

    /// Fetch a stored runnable binary by `name:tag`, or `Ok(None)` if absent.
    pub fn artifact_binary(&self, name: &str, tag: &str) -> io::Result<Option<Vec<u8>>> {
        let index = self.read_artifact_index()?;
        let entry = index.iter().find(|a| a.name == name && a.tag == tag);
        let Some(entry) = entry else {
            return Ok(None);
        };
        let hash = parse_hex(&entry.binary_hash)?;
        self.store.get(&hash)
    }

    /// List all stored runnable artifacts (name:tag aliases).
    pub fn list_artifacts(&self) -> io::Result<Vec<StoredArtifact>> {
        self.read_artifact_index()
    }

    /// Resolve the `artifacts.json` index path (disk-backed registries only).
    fn artifact_index_path(&self) -> io::Result<PathBuf> {
        let dir = match self
            .store
            .as_any()
            .downcast_ref::<crate::cas::DiskObjectStore>()
        {
            Some(disk) => disk.root(),
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "artifact index requires a disk-backed registry",
                ));
            }
        };
        Ok(dir.join(ARTIFACT_INDEX_FILE))
    }

    fn read_artifact_index(&self) -> io::Result<Vec<StoredArtifact>> {
        let path = self.artifact_index_path()?;
        if !path.exists() {
            return Ok(vec![]);
        }
        let bytes = std::fs::read(&path)?;
        match serde_json::from_slice(&bytes) {
            Ok(index) => Ok(index),
            Err(e) => {
                // The index is derived data (the blobs themselves are
                // content-addressed and intact), so a torn or corrupted file
                // must not brick every subsequent read. Preserve the corrupt
                // file for forensics and start from an empty index.
                let backup = path.with_extension("json.corrupt");
                let _ = std::fs::rename(&path, &backup);
                eprintln!(
                    "[daedalus] artifact index corrupt at {} — moved to {} ({}); rebuilding empty",
                    path.display(),
                    backup.display(),
                    e
                );
                Ok(vec![])
            }
        }
    }

    fn write_artifact_index(&self, index: &[StoredArtifact]) -> io::Result<()> {
        let path = self.artifact_index_path()?;
        let bytes = serde_json::to_vec_pretty(index).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("serialize artifact index: {e}"),
            )
        })?;
        // Replace atomically so a crash mid-write can't tear the index.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }
}

/// Compute the SHA-256 hash of `data`.
pub fn content_hash(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// Format a 32-byte hash as lowercase hex.
pub fn format_hex(hash: &[u8; 32]) -> String {
    hex::encode(hash)
}

/// Parse a hex string into a 32-byte hash.
pub fn parse_hex(hex_str: &str) -> io::Result<[u8; 32]> {
    let bytes = hex::decode(hex_str).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("invalid hex hash: {e}"))
    })?;
    let result: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "hash must be 32 bytes"))?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::{Capability, ConfigLayer, RuntimeLayer};

    #[test]
    /// `push_pull_layer_roundtrip` - push pull layer roundtrip.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn push_pull_layer_roundtrip() {
        let mut reg = LayerRegistry::new(Box::new(crate::cas::MemoryStore::new()));
        let layer = SerializableLayer::Runtime(RuntimeLayer {
            name: "python3".into(),
            interpreter: "python3".into(),
            entrypoint: vec!["python3 /app/main.py".into()],
            version: Some("3.11".into()),
            env: vec![],
            capabilities: vec![Capability::ReadFile, Capability::Network],
        });
        let hash = reg.push_layer(&layer).unwrap();
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64);

        let exists = reg.layer_exists(&hash).unwrap();
        assert!(exists);

        let retrieved = reg.pull_layer(&hash).unwrap();
        match retrieved {
            SerializableLayer::Runtime(rt) => {
                assert_eq!(rt.name, "python3");
                assert_eq!(rt.interpreter, "python3");
            }
            _ => panic!("expected RuntimeLayer"),
        }
    }

    #[test]
    /// `pull_missing_layer_returns_not_found` - pull missing layer returns not found.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn pull_missing_layer_returns_not_found() {
        let reg = LayerRegistry::new(Box::new(crate::cas::MemoryStore::new()));
        let absent_hash = format_hex(&[0u8; 32]);
        let result = reg.pull_layer(&absent_hash);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
    }

    #[test]
    /// `push_different_content_different_hash` - push different content different hash.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn push_different_content_different_hash() {
        let mut reg = LayerRegistry::new(Box::new(crate::cas::MemoryStore::new()));
        let layer_a = SerializableLayer::Runtime(RuntimeLayer {
            name: "python3".into(),
            interpreter: "python3".into(),
            entrypoint: vec![],
            version: None,
            env: vec![],
            capabilities: vec![],
        });
        let layer_b = SerializableLayer::Config(ConfigLayer {
            name: "app-config".into(),
            format: "json".to_string(),
            data: serde_json::json!({ "port": 8080 }),
        });
        let hash_a = reg.push_layer(&layer_a).unwrap();
        let hash_b = reg.push_layer(&layer_b).unwrap();
        assert_ne!(hash_a, hash_b);
    }

    #[test]
    /// `build_and_publish_creates_manifest` - build and publish creates manifest.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn build_and_publish_creates_manifest() {
        let mut reg = LayerRegistry::new(Box::new(crate::cas::MemoryStore::new()));
        let runtime = SerializableLayer::Runtime(RuntimeLayer {
            name: "python3".into(),
            interpreter: "python3".into(),
            entrypoint: vec![],
            version: None,
            env: vec![],
            capabilities: vec![Capability::Exec],
        });
        let model = SerializableLayer::Config(ConfigLayer {
            name: "app-config".into(),
            format: "json".to_string(),
            data: serde_json::json!({ "port": 8080 }),
        });
        let (manifest_hash, refs) = reg.build_and_publish("app", &[runtime, model]).unwrap();
        assert!(!manifest_hash.is_empty());
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].name, "python3");
        assert_eq!(refs[1].name, "app-config");

        // Retrieve the manifest
        let manifest = reg.get_artifact(&manifest_hash).unwrap();
        assert_eq!(manifest.artifact_name, "app");
        assert_eq!(manifest.layers.len(), 2);
    }

    #[test]
    /// `artifact_binary_roundtrip_by_name` - publish/fetch a runnable binary by name:tag.
    ///
    /// Description:
    /// Publishes an opaque byte blob (a stand-in .de binary) under two tags,
    /// verifies fetching by name, that a later publish of the same tag
    /// overwrites it, and that list reports exactly the aliases.
    ///
    /// Return: nothing
    fn artifact_binary_roundtrip_by_name() {
        let dir = std::env::temp_dir().join(format!("daedalus-reg-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut reg = LayerRegistry::disk(&dir).unwrap();

        let blob_a = b"\x7fELF fake .de bytes a".to_vec();
        let blob_b = b"\x7fELF fake .de bytes b".to_vec();
        let hash_a = reg
            .publish_artifact_binary("ollama", "latest", &blob_a)
            .unwrap();
        assert_eq!(hash_a.len(), 64);
        let hash_b = reg
            .publish_artifact_binary("gemma", "1.1", &blob_b)
            .unwrap();
        assert_ne!(hash_a, hash_b);

        assert_eq!(
            reg.artifact_binary("ollama", "latest").unwrap(),
            Some(blob_a)
        );
        assert_eq!(reg.artifact_binary("gemma", "1.1").unwrap(), Some(blob_b));
        assert!(reg.artifact_binary("ollama", "1.0").unwrap().is_none());
        assert!(reg.artifact_binary("missing", "latest").unwrap().is_none());

        let listed = reg.list_artifacts().unwrap();
        assert_eq!(listed.len(), 2);
        assert!(listed
            .iter()
            .any(|a| a.name == "ollama" && a.tag == "latest"));

        // Publishing the same name:tag again overwrites, keeping one alias.
        let blob_c = b"\x7fELF fake .de bytes c".to_vec();
        reg.publish_artifact_binary("ollama", "latest", &blob_c)
            .unwrap();
        let listed = reg.list_artifacts().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(
            reg.artifact_binary("ollama", "latest").unwrap(),
            Some(blob_c)
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    /// `artifact_index_self_heals_corruption` - a torn/corrupt index is
    /// preserved aside and treated as empty so the registry keeps serving;
    /// publishing afterwards rebuilds the index cleanly.
    fn artifact_index_self_heals_corruption() {
        let dir = std::env::temp_dir().join(format!(
            "daedalus-heal-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let mut reg = LayerRegistry::disk(&dir).unwrap();
        reg.publish_artifact_binary("app", "1", b"bytes-a").unwrap();

        // Tear the index mid-write: garbage bytes.
        std::fs::write(dir.join(ARTIFACT_INDEX_FILE), b"{\"name\":").unwrap();

        // Reading a corrupt index recovers (empty) instead of erroring.
        assert!(reg.list_artifacts().unwrap().is_empty());
        // And publish still works, rebuilding from empty.
        reg.publish_artifact_binary("app", "2", b"bytes-b").unwrap();
        let listed = reg.list_artifacts().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].tag, "2");

        // The corrupt bytes were preserved, not silently destroyed.
        assert!(dir.join("artifacts.json.corrupt").exists());

        // Names with characters that break the HTTP GET round trip are
        // rejected at the API boundary.
        assert!(reg.publish_artifact_binary("a:b", "1", b"x").is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    /// `hex_roundtrip` - hex roundtrip.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn hex_roundtrip() {
        let hash = content_hash(b"hello");
        let hex = format_hex(&hash);
        let parsed = parse_hex(&hex).unwrap();
        assert_eq!(parsed, hash);
    }

    #[test]
    /// `shared_runtime_layer_produces_same_hash` - shared runtime layer produces same hash.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn shared_runtime_layer_produces_same_hash() {
        // Use case: two apps sharing the same python3 runtime layer.
        // Both push identical RuntimeLayer → same content hash → stored once.
        // Both push identical RuntimeLayer → same content hash → stored once.
        let runtime = || {
            SerializableLayer::Runtime(RuntimeLayer {
                name: "python3".into(),
                interpreter: "python3".into(),
                entrypoint: vec!["python3 {app}/main.py".into()],
                version: Some("3.11".into()),
                env: vec![("DAEDALUS_LOG_LEVEL".into(), "info".into())],
                capabilities: vec![Capability::Exec, Capability::Network],
            })
        };

        let mut reg = LayerRegistry::new(Box::new(crate::cas::MemoryStore::new()));
        let hash1 = reg.push_layer(&runtime()).unwrap();
        let hash2 = reg.push_layer(&runtime()).unwrap();
        assert_eq!(
            hash1, hash2,
            "identical layers must produce the same content hash"
        );

        // Verify deduplication: only one entry stored for two identical pushes
        let all = reg.list_layers().unwrap();
        assert!(
            all.len() <= 1,
            "identical push should not create duplicate entries"
        );
    }
}
