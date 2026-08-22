//! Metadata structures for .daedalus files.
//!
//! Contains configuration for embedded runtimes, build caching, health checks, and WASM support.
//!
//! NOTE: the on-disk artifact metadata JSON is produced by
//! `crate::assembly::build_meta_json` as a raw `serde_json::Value`; the types
//! here are the shared config/build-time structures, not the artifact format.

use serde::{Deserialize, Serialize};

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
}
