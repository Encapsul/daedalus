//! MCP (Model Context Protocol) server for the daedalus stub.
//!
//! When launched with `DAEDALUS_MCP=1` and the binary carries embedded MCP
//! tools, the stub exposes them over stdin/stdout JSON-RPC 2.0 instead of
//! exec'ing the app. This lets a model/agent drive the packaged tools.
//!
//! Transport: newline-delimited JSON — one JSON-RPC message per line on both
//! stdin (requests) and stdout (responses). This is a simplification of the
//! official MCP stdio framing (Content-Length headers, as in LSP); it keeps
//! the launcher tiny and is sufficient for the documented stdin/stdout
//! JSON-RPC contract. Each line is parsed and answered independently.

use crate::Metadata;
use daedalus_core::mcp::McpTool;
use std::io::{self, BufRead, Write};

/// Runs the MCP server: reads requests line-by-line from stdin, writes each
/// response line to stdout, and never returns until stdin closes.
///
/// `tools` must already be resolved to absolute paths the stub can execute.
pub fn run_mcp_server(tools: &[McpTool], stdout: &mut dyn Write) -> io::Result<()> {
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(response) = handle_line(trimmed, tools) {
            writeln!(stdout, "{response}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

/// Resolve the embedded (rootfs-relative) tool commands to absolute paths.
///
/// `command[0]` is stored as `tools/<name>`; joining it onto the extracted
/// rootfs yields the runnable script. Absolute commands are left untouched.
pub fn resolve_tool_commands(tools: &[McpTool], rootfs: &std::path::Path) -> Vec<McpTool> {
    tools
        .iter()
        .map(|tool| McpTool {
            name: tool.name.clone(),
            command: if tool.command[0].starts_with('/') {
                tool.command.clone()
            } else {
                vec![rootfs.join(&tool.command[0]).to_string_lossy().into_owned()]
            },
        })
        .collect()
}

/// Whether the app should run as an MCP server for these tools.
pub fn mcp_mode(meta: &Metadata) -> bool {
    meta.mcp_tools
        .as_ref()
        .is_some_and(|_| std::env::var("DAEDALUS_MCP").is_ok_and(|v| v == "1"))
}

/// Parse and respond to a single JSON-RPC request line.
///
/// Supported methods: `initialize`, `notifications` (no response),
/// `tools/list`, and `tools/call`. Returns the response line, or `None` for
/// notifications and malformed input. Separated from the stdio loop so it is
/// unit-testable without real stdin/stdout.
fn handle_line(line: &str, tools: &[McpTool]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let method = value.get("method")?.as_str()?;
    let id = value.get("id");
    match method {
        "initialize" => Some(response(
            id,
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "daedalus", "version": env!("CARGO_PKG_VERSION") },
            }),
        )),
        "notifications/initialized" | "notifications/cancelled" => None,
        "tools/list" => {
            let listed: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.name,
                        "description": "",
                        "inputSchema": { "type": "object", "properties": {} },
                    })
                })
                .collect();
            Some(response(id, serde_json::json!({ "tools": listed })))
        }
        "tools/call" => call_tool(&value, id, tools),
        _ => Some(response_error(
            id,
            -32601,
            format!("method not found: {method}"),
        )),
    }
}

/// Run one `tools/call`: look up the tool, spawn its command, pass the params
/// on stdin, and return the captured stdout, or a JSON-RPC error on failure.
fn call_tool(
    value: &serde_json::Value,
    id: Option<&serde_json::Value>,
    tools: &[McpTool],
) -> Option<String> {
    let name = value.get("params")?.get("name")?.as_str()?;
    let params = value.get("params").and_then(|p| p.get("arguments"));
    let tool = tools.iter().find(|t| t.name == name)?;
    let mut cmd = tool.command.clone();
    let prog = cmd.remove(0);

    let mut child = std::process::Command::new(&prog)
        .args(cmd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;

    if let Some(ps) = &params {
        if let Ok(input) = serde_json::to_string(ps) {
            use std::io::Write as _;
            if let Some(mut stdin_w) = child.stdin.take() {
                let _ = stdin_w.write_all(input.as_bytes());
            }
        }
    }

    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return Some(response_error(
            id,
            -32000,
            format!(
                "tool '{name}' exited with status {:?}",
                output.status.code()
            ),
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    Some(response(
        id,
        serde_json::json!({ "content": [{ "type": "text", "text": text }], "isError": false }),
    ))
}

/// Build a successful JSON-RPC response object line.
fn response(id: Option<&serde_json::Value>, result: serde_json::Value) -> String {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

/// Build an error JSON-RPC response object line.
fn response_error(id: Option<&serde_json::Value>, code: i64, message: String) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use daedalus_core::mcp::McpTool;

    fn tools() -> Vec<McpTool> {
        vec![McpTool {
            name: "echo".into(),
            command: vec!["cat".into()],
        }]
    }

    #[test]
    fn tools_list_returns_tool_names() {
        let line = handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
            &tools(),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["result"]["tools"][0]["name"], "echo");
        assert_eq!(v["id"], 1);
    }

    #[test]
    fn tools_call_runs_echo_tool() {
        let line = handle_line(
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"echo","arguments":{"x":1}}}"#,
            &tools(),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["result"]["content"][0]["text"], "{\"x\":1}");
        assert_eq!(v["result"]["isError"], false);
    }
}
