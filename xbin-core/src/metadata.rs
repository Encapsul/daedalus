//! Metadata structures for .xbin files.
//!
//! Contains configuration for embedded runtimes, build caching, health checks, and WASM support.

use serde::{Deserialize, Serialize};

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
            EmbeddedInterpreter::Custom(p) => write!(f, "{p}"),
        }
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
            max_entries: 100,
            ttl_hours: Some(24),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EmbeddedRuntimeConfig {
    pub interpreter: Option<EmbeddedInterpreter>,
    pub interpreter_path: Option<String>,
    pub runtime_cache: Option<String>,
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
        assert_eq!(bc.max_entries, 100);
        assert_eq!(bc.ttl_hours, Some(24));
    }

    #[test]
    fn bun_features_builder_pattern() {
        let features = BunFeatures::default()
            .with_embedded_runtime(EmbeddedInterpreter::Python3)
            .with_health_check(8080, Some("/_health".into()));

        assert_eq!(
            features.embedded_runtime.interpreter,
            Some(EmbeddedInterpreter::Python3)
        );
        assert!(features.health_check.enabled);
        assert_eq!(features.health_check.port, 8080);
        assert_eq!(features.health_check.endpoint, "/_health");
    }
}
