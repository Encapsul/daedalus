//! Layer data model for Daedalus artifacts.
//!
//! Defines the serializable layer types stored in artifact metadata JSON.
//! The stub resolves its entrypoint from these layers (`entrypoint_layer` →
//! `StartupLayer`); orchestration traits live with their consumers, not here.

use serde::{Deserialize, Serialize};

/// The kind of layer content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LayerKind {
    /// Language runtime (python, node, deno, java, etc.)
    Runtime,
    /// Configuration/data layer
    Config,
    /// Custom/unknown layer type
    Custom,
}

/// Serializable layer types for metadata storage.
/// This enum allows layers to be stored in the binary metadata JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SerializableLayer {
    Runtime(RuntimeLayer),
    Config(ConfigLayer),
    Custom {
        name: String,
        #[serde(flatten)]
        extra: serde_json::Value,
    },
}

impl SerializableLayer {
    /// Get the layer name.
    pub fn name(&self) -> &str {
        match self {
            SerializableLayer::Runtime(l) => &l.name,
            SerializableLayer::Config(l) => &l.name,
            SerializableLayer::Custom { name, .. } => name,
        }
    }

    /// Get the layer kind.
    pub fn kind(&self) -> LayerKind {
        match self {
            SerializableLayer::Runtime(_) => LayerKind::Runtime,
            SerializableLayer::Config(_) => LayerKind::Config,
            SerializableLayer::Custom { .. } => LayerKind::Custom,
        }
    }
}

impl From<RuntimeLayer> for SerializableLayer {
    /// from - from.
    /// @layer: layer
    ///
    /// Description:
    ///
    /// Return: the Self
    fn from(layer: RuntimeLayer) -> Self {
        SerializableLayer::Runtime(layer)
    }
}

impl From<ConfigLayer> for SerializableLayer {
    /// from - from.
    /// @layer: layer
    ///
    /// Description:
    ///
    /// Return: the Self
    fn from(layer: ConfigLayer) -> Self {
        SerializableLayer::Config(layer)
    }
}

/// Encryption metadata for a layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerEncryption {
    pub algorithm: String,      // e.g., "aes-256-gcm"
    pub key_id: Option<String>, // key identifier for key management
    pub nonce: [u8; 12],        // fixed 12-byte nonce for AES-GCM
}

impl LayerEncryption {
    /// new - new.
    /// @algorithm: algorithm
    /// @key_id: key id
    /// @nonce: nonce
    ///
    /// Description:
    ///
    /// Return: the Self
    pub fn new(algorithm: String, key_id: Option<String>, nonce: [u8; 12]) -> Self {
        Self {
            algorithm,
            key_id,
            nonce,
        }
    }
}

/// Capabilities a layer may request at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    ReadFile,
    WriteFile,
    Network,
    Exec,
    Syscall,
    Env,
}

/// Concrete runtime layer (Python, Node, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeLayer {
    pub name: String,
    pub interpreter: String,     // bare name: "python3", "node", etc.
    pub entrypoint: Vec<String>, // argv template with {app} placeholder
    pub version: Option<String>,
    pub env: Vec<(String, String)>,
    pub capabilities: Vec<Capability>,
}

/// Config layer (JSON, YAML, TOML configuration)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigLayer {
    pub name: String,
    pub format: String, // "json", "yaml", "toml"
    pub data: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// runtime_layer_serialization - runtime layer serialization.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn runtime_layer_serialization() {
        let layer = RuntimeLayer {
            name: "python3".into(),
            interpreter: "python3".into(),
            entrypoint: vec!["{app}/main.py".into()],
            version: Some("3.11".into()),
            env: vec![],
            capabilities: vec![Capability::ReadFile, Capability::Network],
        };
        let json = serde_json::to_string(&layer).unwrap();
        let parsed: RuntimeLayer = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, layer.name);
        assert_eq!(parsed.interpreter, layer.interpreter);
    }

    #[test]
    /// runtime_layer_kind - runtime layer kind.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn runtime_layer_kind() {
        let layer = RuntimeLayer {
            name: "python3".into(),
            interpreter: "python3".into(),
            entrypoint: vec![],
            version: None,
            env: vec![],
            capabilities: vec![],
        };
        assert_eq!(SerializableLayer::from(layer).kind(), LayerKind::Runtime);
    }

    #[test]
    /// serializable_layer_name_and_custom_roundtrip - serializable layer name and custom roundtrip.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn serializable_layer_name_and_custom_roundtrip() {
        let custom = SerializableLayer::Custom {
            name: "my-data".into(),
            extra: serde_json::json!({ "bytes": 12 }),
        };
        assert_eq!(custom.name(), "my-data");
        assert_eq!(custom.kind(), LayerKind::Custom);
        let json = serde_json::to_string(&custom).unwrap();
        let parsed: SerializableLayer = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name(), "my-data");
    }

    #[test]
    /// config_layer_roundtrip - config layer roundtrip.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn config_layer_roundtrip() {
        let layer = ConfigLayer {
            name: "app-config".into(),
            format: "toml".to_string(),
            data: serde_json::json!({ "port": 8080 }),
        };
        let json = serde_json::to_string(&layer).unwrap();
        let parsed: ConfigLayer = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, layer.name);
        assert_eq!(parsed.format, layer.format);
    }

    #[test]
    /// layer_encryption_nonce_fixed_size - layer encryption nonce fixed size.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn layer_encryption_nonce_fixed_size() {
        let nonce = [0x42u8; 12];
        let enc = LayerEncryption::new("aes-256-gcm".into(), None, nonce);
        assert_eq!(enc.nonce.len(), 12);
        assert_eq!(enc.algorithm, "aes-256-gcm");
    }
}
