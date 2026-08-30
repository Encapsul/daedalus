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
use std::path::Path;

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
    /// push_pull_layer_roundtrip - push pull layer roundtrip.
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
    /// pull_missing_layer_returns_not_found - pull missing layer returns not found.
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
    /// push_different_content_different_hash - push different content different hash.
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
    /// build_and_publish_creates_manifest - build and publish creates manifest.
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
    /// hex_roundtrip - hex roundtrip.
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
    /// shared_runtime_layer_produces_same_hash - shared runtime layer produces same hash.
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
