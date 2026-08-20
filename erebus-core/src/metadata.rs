//! Metadata structures for .erebus files.
//!
//! Contains configuration for embedded runtimes, build caching, health checks, and WASM support.

use serde::{Deserialize, Serialize};

use crate::layer::{LayerKind, SerializableLayer};

/// Metadata for the entire artifact (new format with layers).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArtifactMetadata {
    /// Application name.
    pub name: String,
    /// erebus version that created this artifact.
    pub erebus_version: String,
    /// Creation timestamp (RFC3339).
    pub created: String,
    /// Layers that compose this artifact.
    #[serde(default)]
    pub layers: Vec<SerializableLayer>,
    /// Name of the layer that contains the main entrypoint.
    #[serde(default)]
    pub entrypoint_layer: Option<String>,
    /// Fallback runtime (for backward compatibility).
    #[serde(default)]
    pub runtime: String,
    /// Fallback entrypoint (for backward compatibility).
    #[serde(default)]
    pub entrypoint: Vec<String>,
    /// Environment variables.
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    /// Working directory.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Isolation level.
    #[serde(default)]
    pub isolation: u8,
    /// Seccomp sandbox enabled.
    #[serde(default)]
    pub seccomp: bool,
    /// Landlock LSM filesystem sandbox enabled.
    #[serde(default)]
    pub landlock: bool,
    /// GUI app — bind-mount X11/Wayland/GPU before `pivot_root`.
    #[serde(default)]
    pub gui: bool,
    /// Payload format (zstd-tar, squashfs).
    #[serde(default)]
    pub payload_format: String,
    /// Health check configuration.
    #[serde(default)]
    pub health_check: Option<HealthCheck>,
    /// Update URL for SISR.
    #[serde(default)]
    pub update_url: Option<String>,
    /// Crypto metadata for encrypted payloads.
    #[serde(default)]
    pub crypto: Option<CryptoMeta>,
    /// Services to run alongside the main entrypoint.
    #[serde(default)]
    pub services: Vec<Service>,
    /// Application version.
    #[serde(default)]
    pub version: Option<String>,
    /// Author.
    #[serde(default)]
    pub author: Option<String>,
    /// Description.
    #[serde(default)]
    pub description: Option<String>,
    /// License.
    #[serde(default)]
    pub license: Option<String>,
    /// Application hash for cache invalidation.
    #[serde(default)]
    pub app_hash: Option<String>,
    /// Runtime dependencies hash.
    #[serde(default)]
    pub rt_deps_hash: Option<String>,
    /// Cross-compile targets.
    #[serde(default)]
    pub cross_compile_targets: Vec<String>,
}

impl ArtifactMetadata {
    /// Get the main entrypoint layer if specified and found.
    pub fn get_entrypoint_layer(&self) -> Option<&SerializableLayer> {
        self.entrypoint_layer
            .as_ref()
            .and_then(|name| self.layers.iter().find(|l| l.name() == name))
    }

    /// Get all layers of a specific kind.
    pub fn layers_of_kind(&self, kind: LayerKind) -> Vec<&SerializableLayer> {
        self.layers.iter().filter(|l| l.kind() == kind).collect()
    }

    /// Get runtime layers (for backward compatibility).
    pub fn runtime_layers(&self) -> Vec<&SerializableLayer> {
        self.layers_of_kind(LayerKind::Runtime)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    pub port: u16,
    pub endpoint: String,
    pub enabled: bool,
}

impl Default for HealthCheck {
    fn default() -> Self {
        Self {
            port: 0,
            endpoint: "/health".to_string(),
            enabled: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WasmConfig {
    pub enabled: bool,
    pub wasmtime_path: Option<String>,
    pub wasi: bool,
    pub component_model: bool,
    pub wasi_fs_map: Vec<WasiFsMap>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasiFsMap {
    pub guest: String,
    pub host: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::derivable_impls)]
pub struct BuildCacheConfig {
    pub enabled: bool,
    pub max_entries: usize,
    pub ttl_hours: Option<u64>,
}

#[allow(clippy::derivable_impls)]
impl Default for BuildCacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_entries: 50,
            ttl_hours: Some(24),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EmbeddedRuntimeConfig {
    pub interpreter: Option<EmbeddedInterpreter>,
    pub interpreter_path: Option<String>,
    pub runtime_cache: Option<String>,
    pub runtime_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::derivable_impls)]
pub struct BunFeatures {
    pub embedded_runtime: EmbeddedRuntimeConfig,
    pub build_cache: BuildCacheConfig,
    pub health_check: HealthCheck,
    pub wasm: WasmConfig,
    pub cross_compile_targets: Vec<String>,
}

#[allow(clippy::derivable_impls)]
impl Default for BunFeatures {
    fn default() -> Self {
        Self {
            embedded_runtime: EmbeddedRuntimeConfig::default(),
            build_cache: BuildCacheConfig::default(),
            health_check: HealthCheck::default(),
            wasm: WasmConfig::default(),
            cross_compile_targets: Vec::new(),
        }
    }
}

impl BunFeatures {
    #[must_use]
    pub fn with_embedded_runtime(mut self, interpreter: EmbeddedInterpreter) -> Self {
        self.embedded_runtime.interpreter = Some(interpreter);
        self
    }

    #[must_use]
    pub fn with_health_check(mut self, port: u16, endpoint: Option<String>) -> Self {
        self.health_check.enabled = true;
        self.health_check.port = port;
        if let Some(ep) = endpoint {
            self.health_check.endpoint = ep;
        }
        self
    }

    #[must_use]
    pub fn with_wasm(mut self, wasmtime_path: Option<String>) -> Self {
        self.wasm.enabled = true;
        self.wasm.wasmtime_path = wasmtime_path;
        self
    }

    #[must_use]
    pub fn with_cross_compile(mut self, targets: Vec<String>) -> Self {
        self.cross_compile_targets = targets;
        self
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Some(EmbeddedInterpreter::Custom(path)) = &self.embedded_runtime.interpreter {
            if path.is_empty() {
                return Err("Custom interpreter path cannot be empty".to_string());
            }
            let path = std::path::Path::new(path);
            if path.exists() && !std::fs::metadata(path).map(|m| m.is_file()).unwrap_or(true) {
                return Err(format!(
                    "Custom interpreter path is not a file: {}",
                    path.display()
                ));
            }
        }

        if self.health_check.enabled {
            if self.health_check.port == 0 {
                return Err(
                    "Health check port cannot be 0 - specify a valid port (1-65535)".to_string(),
                );
            }
            if self.health_check.endpoint.is_empty() {
                return Err("Health check endpoint cannot be empty".to_string());
            }
            if !self.health_check.endpoint.starts_with('/') {
                return Err(format!(
                    "Health check endpoint must start with '/': {}",
                    self.health_check.endpoint
                ));
            }
        }

        if self.wasm.enabled {
            if let Some(ref path) = self.wasm.wasmtime_path {
                let p = std::path::Path::new(path);
                if !p.exists() {
                    return Err(format!("wasmtime binary not found at: {}", path));
                }
            }
        }

        if self.build_cache.enabled && self.build_cache.max_entries > 1000 {
            return Err(format!("Build cache max_entries {} is too high - use 1000 or less to prevent memory issues", self.build_cache.max_entries));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EmbeddedInterpreter {
    Python3,
    Node,
    Deno,
    Ruby,
    Php,
    Perl,
    Java,
    Go,
    Wasm,
    Electron,
    Custom(String),
}

impl std::fmt::Display for EmbeddedInterpreter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmbeddedInterpreter::Python3 => write!(f, "python3"),
            EmbeddedInterpreter::Node => write!(f, "node"),
            EmbeddedInterpreter::Deno => write!(f, "deno"),
            EmbeddedInterpreter::Ruby => write!(f, "ruby"),
            EmbeddedInterpreter::Php => write!(f, "php"),
            EmbeddedInterpreter::Perl => write!(f, "perl"),
            EmbeddedInterpreter::Java => write!(f, "java"),
            EmbeddedInterpreter::Go => write!(f, "go"),
            EmbeddedInterpreter::Wasm => write!(f, "wasm"),
            EmbeddedInterpreter::Electron => write!(f, "electron"),
            EmbeddedInterpreter::Custom(p) => write!(f, "{p}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CryptoMeta {
    pub nonce_hex: String,
    pub tag_offset: usize,
    pub encryption_key_hex: String,
    pub encryption_salt_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub name: String,
    pub command: Vec<String>,
    pub env: std::collections::BTreeMap<String, String>,
    pub health_check: Option<HealthCheck>,
    pub restart: RestartPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum RestartPolicy {
    #[default]
    Never,
    OnFailure,
    Always,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_interpreter_display() {
        assert_eq!(EmbeddedInterpreter::Python3.to_string(), "python3");
        assert_eq!(EmbeddedInterpreter::Node.to_string(), "node");
        assert_eq!(
            EmbeddedInterpreter::Custom("/path/to/bin".into()).to_string(),
            "/path/to/bin"
        );
    }

    #[test]
    fn health_check_defaults() {
        let hc = HealthCheck::default();
        assert_eq!(hc.port, 0);
        assert_eq!(hc.endpoint, "/health");
        assert!(!hc.enabled);
    }

    #[test]
    fn wasm_config_defaults() {
        let wc = WasmConfig::default();
        assert!(!wc.enabled);
    }

    #[test]
    fn build_cache_defaults() {
        let bc = BuildCacheConfig::default();
        assert!(bc.enabled);
        assert_eq!(bc.max_entries, 50);
        assert_eq!(bc.ttl_hours, Some(24));
    }

    #[test]
    fn artifact_metadata_layers() {
        use crate::layer::{Capability, LayerKind, RuntimeLayer, SerializableLayer};

        let runtime_layer = RuntimeLayer {
            name: "python3".into(),
            interpreter: "python3".into(),
            entrypoint: vec!["{app}/main.py".into()],
            version: Some("3.11".into()),
            env: vec![],
            capabilities: vec![Capability::ReadFile, Capability::Network],
        };

        let metadata = ArtifactMetadata {
            name: "test-app".into(),
            erebus_version: "0.5.0".into(),
            created: "2024-01-01T00:00:00Z".into(),
            layers: vec![SerializableLayer::Runtime(runtime_layer)],
            entrypoint_layer: Some("python3".into()),
            ..Default::default()
        };

        // Test get_entrypoint_layer
        let entry_layer = metadata.get_entrypoint_layer().unwrap();
        assert_eq!(entry_layer.name(), "python3");
        assert_eq!(entry_layer.kind(), LayerKind::Runtime);

        // Test layers_of_kind
        let runtime_layers = metadata.layers_of_kind(LayerKind::Runtime);
        assert_eq!(runtime_layers.len(), 1);

        let model_layers = metadata.layers_of_kind(LayerKind::Model);
        assert_eq!(model_layers.len(), 0);
    }

    #[test]
    fn artifact_metadata_serialization() {
        use crate::layer::{RuntimeLayer, SerializableLayer};

        let runtime_layer = RuntimeLayer {
            name: "node".into(),
            interpreter: "node".into(),
            entrypoint: vec!["{app}/index.js".into()],
            version: Some("20".into()),
            env: vec![],
            capabilities: vec![],
        };

        let metadata = ArtifactMetadata {
            name: "test-app".into(),
            erebus_version: "0.5.0".into(),
            created: "2024-01-01T00:00:00Z".into(),
            layers: vec![SerializableLayer::Runtime(runtime_layer)],
            entrypoint_layer: Some("node".into()),
            ..Default::default()
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let parsed: ArtifactMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.name, metadata.name);
        assert_eq!(parsed.layers.len(), 1);
        assert_eq!(parsed.entrypoint_layer, metadata.entrypoint_layer);
    }
}
