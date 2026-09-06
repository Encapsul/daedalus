use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use daedalus_core::registry::LayerRegistry;

#[derive(Args)]
pub struct ServeArgs {
    #[command(subcommand)]
    pub command: ServeCommand,
}

#[derive(Subcommand)]
pub enum ServeCommand {
    /// Start a local registry server
    Start(ServeStartArgs),
}

#[derive(Args)]
pub struct ServeStartArgs {
    /// Address to bind to (default: 127.0.0.1:8080)
    #[arg(short, long, default_value = "127.0.0.1:8080")]
    pub bind: String,

    /// Directory to store layers (default: ~/.daedalus/registry)
    #[arg(short, long, default_value = "~/.daedalus/registry")]
    pub dir: PathBuf,

    /// Authentication token (Bearer token)
    #[arg(long, env = "DAEDALUS_TOKEN")]
    pub token: Option<String>,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Output result as JSON
    #[arg(long)]
    pub json: bool,
}

/// run - dispatch a serve subcommand.
/// @args: command arguments
///
/// Description:
/// Currently supports the `start` subcommand to launch a local registry HTTP server.
///
/// Return: Result containing Result<()>
pub fn run(args: ServeArgs) -> Result<()> {
    match args.command {
        ServeCommand::Start(sub) => run_start(sub),
    }
}

/// run_start - start a local layer registry HTTP server.
/// @args: command arguments
///
/// Description:
/// Binds a TcpListener and spawns a thread per connection handling GET /list,
/// GET /pull/<hash>, and POST /push.
///
/// Return: Result containing Result<()>
fn run_start(args: ServeStartArgs) -> Result<()> {
    let dir = expand_tilde(&args.dir);
    std::fs::create_dir_all(&dir).context("failed to create registry directory")?;
    let reg = LayerRegistry::disk(&dir).context("failed to init registry")?;
    let reg = Arc::new(Mutex::new(reg));

    let listener = TcpListener::bind(&args.bind).context("failed to bind address")?;
    if args.json {
        let info = serde_json::json!({
            "command": "serve",
            "subcommand": "start",
            "bind": args.bind,
            "dir": dir.display().to_string(),
            "success": true,
        });
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        eprintln!("[daedalus] registry server listening on {}", args.bind);
        eprintln!("[daedalus] storage: {}", dir.display());
    }

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let token = args.token.clone();
                let verbose = args.verbose;
                let reg = Arc::clone(&reg);
                thread::spawn(move || {
                    if let Err(e) =
                        handle_connection(stream, &mut reg.lock().unwrap(), token, verbose)
                    {
                        eprintln!("[daedalus] error: {e}");
                    }
                });
            }
            Err(e) => {
                eprintln!("[daedalus] connection error: {e}");
            }
        }
    }

    Ok(())
}

/// handle_connection - parse an HTTP request and dispatch to the handler.
///
/// Description:
/// Reads the request head (up to a blank line), then any `Content-Length`
/// body bytes that follow, and routes to the matching handler.
///
/// Return: nothing
fn handle_connection(
    mut stream: TcpStream,
    reg: &mut LayerRegistry,
    token: Option<String>,
    verbose: bool,
) -> Result<()> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    let mut head_bytes = Vec::new();
    let mut body = Vec::new();
    let mut split_idx = None;
    loop {
        let mut buf = [0u8; 8192];
        let n = stream.read(&mut buf)?;
        if n == 0 {
            break;
        }
        head_bytes.extend_from_slice(&buf[..n]);
        if let Some(idx) = find_head_end(&head_bytes) {
            split_idx = Some(idx);
            body = head_bytes.split_off(idx);
            break;
        }
    }
    let Some(head_end) = split_idx else {
        send_response(&mut stream, 400, "Bad Request", "Incomplete request head")?;
        return Ok(());
    };

    let head_str = String::from_utf8_lossy(&head_bytes);
    let first_line = head_str.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        send_response(&mut stream, 400, "Bad Request", "Invalid request")?;
        return Ok(());
    }

    let method = parts[0];
    let path = parts[1];

    if verbose {
        eprintln!("[daedalus] {} {}", method, path);
    }

    // Read the declared body even when it spans multiple TCP reads.
    let content_length = content_length(&head_str);
    let mut buffer = std::io::Cursor::new(body);
    let remaining = content_length.saturating_sub(buffer.get_ref().len());
    if remaining > 0 {
        let mut rest = vec![0u8; remaining];
        stream.read_exact(&mut rest)?;
        buffer.get_mut().extend_from_slice(&rest);
    }
    let body = buffer.into_inner();

    let (path, query) = match path.split_once('?') {
        Some((p, q)) => (p, q),
        None => (path, ""),
    };

    match (method, path) {
        ("GET", "/list") => handle_list(&mut stream, reg)?,
        ("GET", "/artifacts") => handle_artifacts(&mut stream, reg)?,
        ("GET", path) if path.starts_with("/pull/") => {
            let hash = &path[6..];
            handle_pull(&mut stream, reg, hash, verbose)?;
        }
        ("GET", path) if path.starts_with("/artifact/") => {
            let spec = &path["/artifact/".len()..];
            handle_artifact_get(&mut stream, reg, spec, verbose)?;
        }
        ("POST", "/push") => handle_push(&mut stream, reg, token, verbose, &body)?,
        ("POST", "/artifact") => {
            handle_artifact_push(&mut stream, reg, token, verbose, &body, query)?;
        }
        _ => send_response(&mut stream, 404, "Not Found", "Endpoint not found")?,
    }

    Ok(())
}

/// find_head_end - locate the end of the HTTP head (`\r\n\r\n` or `\n\n`).
///
/// Return: byte index just past the head, or None if the head is incomplete.
fn find_head_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
        .or_else(|| bytes.windows(2).position(|w| w == b"\n\n").map(|i| i + 2))
}

/// content_length - parse the `Content-Length` header value from a request head.
///
/// Return: the declared length, 0 if absent or unparseable.
fn content_length(head: &str) -> usize {
    for line in head.lines() {
        let line = line.trim();
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            return value.trim().parse().unwrap_or(0);
        }
    }
    0
}

/// handle_list - return all layer hashes in the registry as plain text.
/// @stream: stream
/// @reg: reg
///
/// Description:
/// Responds with 200 and a newline-separated list of layer hashes.
///
/// Return: Result containing Result<()>
fn handle_list(stream: &mut TcpStream, reg: &LayerRegistry) -> Result<()> {
    let layers = reg.list_layers().unwrap_or_default();
    let body = layers.join("\n");
    send_response(stream, 200, "OK", &body)
}

/// handle_pull - return a layer's JSON by hash.
///
/// Description:
/// Looks up the hash in the registry and returns the serialized layer as
/// application/json, or 404 if not found.
///
/// Return: nothing
fn handle_pull(
    stream: &mut TcpStream,
    reg: &LayerRegistry,
    hash: &str,
    verbose: bool,
) -> Result<()> {
    match reg.pull_layer(hash) {
        Ok(layer) => {
            let body = serde_json::to_vec_pretty(&layer)?;
            send_response_bytes(stream, 200, "OK", &body, "application/json")?;
            if verbose {
                eprintln!("[daedalus] pulled layer {hash}");
            }
        }
        Err(_) => {
            send_response(stream, 404, "Not Found", "Layer not found")?;
        }
    }
    Ok(())
}

/// handle_push - accept a layer JSON in the request body and store it.
///
/// Description:
/// Deserializes the request body as a SerializableLayer and stores it in the
/// registry. Returns the layer hash on success.
///
/// Return: nothing
fn handle_push(
    stream: &mut TcpStream,
    reg: &mut LayerRegistry,
    _token: Option<String>,
    verbose: bool,
    body: &[u8],
) -> Result<()> {
    if body.is_empty() {
        send_response(stream, 400, "Bad Request", "Empty body")?;
        return Ok(());
    }

    let layer: daedalus_core::layer::SerializableLayer = match serde_json::from_slice(body) {
        Ok(l) => l,
        Err(e) => {
            send_response(stream, 400, "Bad Request", &format!("Invalid JSON: {e}"))?;
            return Ok(());
        }
    };

    let hash = reg.push_layer(&layer)?;
    if verbose {
        eprintln!("[daedalus] pushed layer '{}' -> {hash}", layer.name());
    }

    send_response(stream, 201, "Created", &hash)
}

/// handle_artifacts - list all stored runnable artifacts as JSON.
///
/// Return: nothing
fn handle_artifacts(stream: &mut TcpStream, reg: &LayerRegistry) -> Result<()> {
    let artifacts = reg.list_artifacts().unwrap_or_default();
    let body = serde_json::to_vec(&artifacts)?;
    send_response_bytes(stream, 200, "OK", &body, "application/json")
}

/// handle_artifact_get - serve a stored `.de` binary by `name:tag`.
///
/// Description:
/// Spec path format: `/artifact/<name>:<tag>`. Responds with the raw binary
/// bytes (content type application/octet-stream) or 404 if absent.
///
/// Return: nothing
fn handle_artifact_get(
    stream: &mut TcpStream,
    reg: &LayerRegistry,
    spec: &str,
    verbose: bool,
) -> Result<()> {
    let Some((name, tag)) = spec.split_once(':') else {
        send_response(stream, 400, "Bad Request", "Expected name:tag")?;
        return Ok(());
    };
    match reg.artifact_binary(name, tag) {
        Ok(Some(bytes)) => {
            send_response_bytes(stream, 200, "OK", &bytes, "application/octet-stream")?;
            if verbose {
                eprintln!(
                    "[daedalus] served artifact '{name}:{tag}' ({} bytes)",
                    bytes.len()
                );
            }
        }
        Ok(None) => {
            send_response(stream, 404, "Not Found", "Artifact not found")?;
        }
        Err(e) => {
            send_response(stream, 500, "Internal Server Error", &e.to_string())?;
        }
    }
    Ok(())
}

/// handle_artifact_push - store a `.de` binary uploaded with `name`/`tag`.
///
/// Description:
/// Query params: `?name=<name>&tag=<tag>` (tag defaults to "latest"). The
/// request body is the full runnable binary; stores it content-addressed and
/// returns the content hash.
///
/// Return: nothing
fn handle_artifact_push(
    stream: &mut TcpStream,
    reg: &mut LayerRegistry,
    _token: Option<String>,
    verbose: bool,
    body: &[u8],
    query: &str,
) -> Result<()> {
    let params: std::collections::HashMap<String, String> = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let Some(name) = params.get("name").filter(|n| !n.is_empty()) else {
        send_response(stream, 400, "Bad Request", "Missing ?name= parameter")?;
        return Ok(());
    };
    let tag = params.get("tag").map(String::as_str).unwrap_or("latest");

    if body.is_empty() {
        send_response(stream, 400, "Bad Request", "Empty body")?;
        return Ok(());
    }

    match reg.publish_artifact_binary(name, tag, body) {
        Ok(hash) => {
            if verbose {
                eprintln!("[daedalus] stored artifact '{name}:{tag}' -> {hash}");
            }
            send_response(stream, 201, "Created", &hash)
        }
        Err(e) => {
            send_response(stream, 500, "Internal Server Error", &e.to_string())?;
            Ok(())
        }
    }
}

/// send_response - write a plain-text HTTP response to a TcpStream.
/// @stream: stream
/// @code: status code
/// @status: status text
/// @body: body
///
/// Description:
/// Formats and sends a minimal HTTP/1.1 response with text/plain content type.
///
/// Return: Result containing Result<()>
fn send_response(stream: &mut TcpStream, code: u16, status: &str, body: &str) -> Result<()> {
    send_response_bytes(stream, code, status, body.as_bytes(), "text/plain")
}

/// send_response_bytes - write a raw-byte HTTP response to a TcpStream.
/// @stream: stream
/// @code: status code
/// @status: status text
/// @body: response body bytes
/// @content_type: MIME content type
///
/// Description:
/// Formats and sends a minimal HTTP/1.1 response with the given content type.
///
/// Return: nothing
fn send_response_bytes(
    stream: &mut TcpStream,
    code: u16,
    status: &str,
    body: &[u8],
    content_type: &str,
) -> Result<()> {
    let headers = format!(
        "HTTP/1.1 {code} {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(headers.as_bytes())?;
    stream.write_all(body)?;
    Ok(())
}

/// expand_tilde - expand a leading ~ to the user's home directory.
/// @path: file or directory path
///
/// Description:
/// Replaces a leading "~/" with $HOME if set; otherwise returns the path unchanged.
///
/// Return: the PathBuf
fn expand_tilde(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    path.to_path_buf()
}
