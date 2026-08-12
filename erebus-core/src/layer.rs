//! Layer and Entrypoint abstractions — Phase 1 of Erebus V2.
//!
//! This module defines the core traits for composable runtime layers.
//! A Layer is a self-contained unit of execution (runtime, model, tool, etc.)
//! An Entrypoint knows how to execute a Layer's content.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

/// The kind of layer content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LayerKind {
    /// Language runtime (python, node, deno, java, etc.)
    Runtime,
    /// AI model weights (GGUF, safetensors, etc.)
    Model,
    /// Tool/plugin binary (WASM, native, etc.)
    Tool,
    /// Configuration/data layer
    Config,
    /// Custom/unknown layer type
    Custom,
}

/// A layer in the Erebus artifact graph.
/// Each layer is content-addressed by its SHA-256 hash.
pub trait Layer: Send + Sync {
    /// Unique identifier for this layer type (e.g. "python3", "llama.cpp", "my-model").
    fn name(&self) -> &str;

    /// Human-readable kind of this layer.
    fn kind(&self) -> LayerKind;

    /// SHA-256 of the layer's payload (content-addressed).
    fn payload_sha256(&self) -> [u8; 32];

    /// Optional: compression algorithm used for this layer's payload.
    fn compression(&self) -> Option<&str> {
        None
    }

    /// Optional: encryption metadata if layer is encrypted.
    fn encryption(&self) -> Option<&LayerEncryption> {
        None
    }

    /// Optional: capabilities required by this layer.
    fn capabilities(&self) -> Vec<Capability> {
        Vec::new()
    }
}

/// Encryption metadata for a layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerEncryption {
    pub algorithm: String,      // e.g., "aes-256-gcm"
    pub key_id: Option<String>, // key identifier for key management
    pub nonce: Vec<u8>,
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

/// Context passed to an Entrypoint during execution.
pub struct ExecutionContext<'a> {
    /// The rootfs path where layers are extracted.
    pub rootfs: &'a Path,
    /// The layer's own directory within rootfs.
    pub layer_dir: &'a Path,
    /// Environment variables for this execution.
    pub env: Vec<(String, String)>,
    /// Arguments passed to the entrypoint.
    pub args: Vec<String>,
    /// Working directory.
    pub cwd: &'a Path,
}

/// An Entrypoint knows how to execute a Layer.
pub trait Entrypoint: Send + Sync {
    /// Returns the layer this entrypoint belongs to.
    fn layer(&self) -> &dyn Layer;

    /// Execute the layer with the given context.
    /// Returns the exit code (0 = success).
    fn execute(&self, ctx: ExecutionContext<'_>) -> io::Result<i32>;

    /// Optional: health check for long-running entrypoints (servers, agents).
    fn health_check(&self, _ctx: &ExecutionContext<'_>) -> io::Result<bool> {
        Ok(true)
    }
}

/// A registry of known Entrypoints, keyed by layer name.
pub struct EntrypointRegistry {
    entrypoints: std::collections::HashMap<String, Box<dyn Entrypoint>>,
}

impl EntrypointRegistry {
    pub fn new() -> Self {
        Self {
            entrypoints: std::collections::HashMap::new(),
        }
    }

    pub fn register(&mut self, entrypoint: Box<dyn Entrypoint>) {
        self.entrypoints
            .insert(entrypoint.layer().name().to_string(), entrypoint);
    }

    pub fn get(&self, name: &str) -> Option<&dyn Entrypoint> {
        self.entrypoints.get(name).map(|e| e.as_ref())
    }

    pub fn names(&self) -> Vec<&String> {
        self.entrypoints.keys().collect()
    }
}

impl Default for EntrypointRegistry {
    fn default() -> Self {
        Self::new()
    }
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

impl Layer for RuntimeLayer {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Runtime
    }

    fn payload_sha256(&self) -> [u8; 32] {
        // Compute from serialized form
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(self.name.as_bytes());
        hasher.update(self.interpreter.as_bytes());
        for arg in &self.entrypoint {
            hasher.update(arg.as_bytes());
        }
        hasher.finalize().into()
    }

    fn capabilities(&self) -> Vec<Capability> {
        self.capabilities.clone()
    }
}

/// Entrypoint for `RuntimeLayer` - uses execvp with the layer's interpreter.
pub struct RuntimeEntrypoint {
    layer: RuntimeLayer,
}

impl RuntimeEntrypoint {
    pub fn new(layer: RuntimeLayer) -> Self {
        Self { layer }
    }
}

impl Entrypoint for RuntimeEntrypoint {
    fn layer(&self) -> &dyn Layer {
        &self.layer
    }

    fn execute(&self, ctx: ExecutionContext<'_>) -> io::Result<i32> {
        // Build argv: interpreter + entrypoint args (with {app} replaced)
        let mut argv = Vec::new();
        argv.push(self.layer.interpreter.clone());
        for arg in &self.layer.entrypoint {
            let replaced = arg.replace("{app}", ctx.rootfs.join("app").to_str().unwrap_or("/app"));
            argv.push(replaced);
        }
        // Add any additional args passed at runtime
        argv.extend(ctx.args);

        // Build environment
        let mut env = ctx.env.clone();
        env.extend(self.layer.env.iter().cloned());

        // Execute using execvp (replaces current process)
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;

            // This never returns on success
            Err(std::process::Command::new(&self.layer.interpreter)
                .args(&argv[1..])
                .envs(env)
                .current_dir(ctx.cwd)
                .exec())
        }

        #[cfg(windows)]
        {
            use std::process::Command;
            let mut cmd = Command::new(&self.layer.interpreter);
            cmd.args(&argv[1..]).envs(env).current_dir(ctx.cwd);
            let status = cmd.status()?;
            Ok(status.code().unwrap_or(-1))
        }
    }
}

/// Model layer (AI model weights)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelLayer {
    pub name: String,
    pub format: String, // "gguf", "safetensors", "onnx", etc.
    pub path: String,   // path within layer directory
    pub size: u64,
    pub metadata: Option<serde_json::Value>,
}

impl Layer for ModelLayer {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Model
    }

    fn payload_sha256(&self) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(self.name.as_bytes());
        hasher.update(self.format.as_bytes());
        hasher.update(self.path.as_bytes());
        hasher.finalize().into()
    }
}

/// Tool layer (WASM plugin, native binary tool)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolLayer {
    pub name: String,
    pub command: Vec<String>, // argv to execute the tool
    pub input_schema: Option<serde_json::Value>,
    pub output_schema: Option<serde_json::Value>,
}

impl Layer for ToolLayer {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Tool
    }

    fn payload_sha256(&self) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(self.name.as_bytes());
        for arg in &self.command {
            hasher.update(arg.as_bytes());
        }
        hasher.finalize().into()
    }
}

/// Config layer (JSON, YAML, TOML configuration)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigLayer {
    pub name: String,
    pub format: String, // "json", "yaml", "toml"
    pub data: serde_json::Value,
}

impl Layer for ConfigLayer {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Config
    }

    fn payload_sha256(&self) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(self.name.as_bytes());
        hasher.update(serde_json::to_vec(&self.data).unwrap_or_default());
        hasher.finalize().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
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
    fn model_layer_kind() {
        let layer = ModelLayer {
            name: "llama-7b".into(),
            format: "gguf".into(),
            path: "models/llama-7b.gguf".into(),
            size: 4_000_000_000,
            metadata: None,
        };
        assert_eq!(layer.kind(), LayerKind::Model);
    }

    #[test]
    fn registry_basic() {
        let mut registry = EntrypointRegistry::new();
        let layer = RuntimeLayer {
            name: "test".into(),
            interpreter: "echo".into(),
            entrypoint: vec![],
            version: None,
            env: vec![],
            capabilities: vec![],
        };
        registry.register(Box::new(RuntimeEntrypoint::new(layer)));
        assert!(registry.get("test").is_some());
        assert!(registry.get("unknown").is_none());
    }
}
