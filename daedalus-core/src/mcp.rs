//! MCP (Model Context Protocol) tool metadata shared by the builder and stub.
//!
//! `daedalus build --mcp-tools <dir>` embeds a directory of standalone tools
//! and records them so the stub can expose them over JSON-RPC when launched
//! in MCP mode (`DAEDALUS_MCP=1`).

use serde::{Deserialize, Serialize};

/// A single MCP tool: an embedded standalone program the runtime can invoke.
///
/// `command` is the argv to execute; the first element is the tool path
/// resolved against the extracted rootfs at launch time.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpTool {
    /// Tool name surfaced via `tools/list` and selected by `tools/call`.
    pub name: String,
    /// argv for the tool. The program is resolved against the rootfs before
    /// the stub's MCP server runs it.
    pub command: Vec<String>,
}

/// MCP tool definitions embedded in `.daedalus` metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct McpToolsMeta {
    /// The ordered list of exported tools.
    pub tools: Vec<McpTool>,
}

/// Build an [`McpToolsMeta`] from a directory of standalone scripts/binaries.
///
/// Every regular file (skipping dotfiles) becomes a tool named after its file
/// name, resolved relative to the rootfs at `tools/<name>`. This keeps the
/// metadata independent of the build host's filesystem paths.
pub fn collect_tools(dir: &std::path::Path) -> std::io::Result<McpToolsMeta> {
    let tools = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            !name.starts_with('.') && name != "Cargo.lock" && name != "Cargo.toml"
        })
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            McpTool {
                name: name.clone(),
                command: vec![format!("tools/{name}")],
            }
        })
        .collect();
    Ok(McpToolsMeta { tools })
}
