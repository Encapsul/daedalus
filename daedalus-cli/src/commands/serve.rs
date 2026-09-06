use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use daedalus_core::http_parse::{
    content_length, head_end, parse_query_params, split_name_tag, split_path_query,
};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use daedalus_core::registry::LayerRegistry;

/// Request heads (headers + first line) are capped to bound memory and the
/// cost of re-scanning a fragmented request head that never terminates.
const MAX_HEAD_BYTES: usize = 64 * 1024;
/// Largest single upload a registry accepts; larger requests get 413
/// immediately instead of tying a worker thread to a bogus Content-Length.
const MAX_BODY_BYTES: u64 = 8 << 30;

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
/// Binds a TcpListener and spawns a thread per connection handling the layer
/// and runnable-artifact endpoints.
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
/// Reads the request head (bounded), then any declared `Content-Length` body
/// bytes, and routes to the matching handler. A request may not read more
/// body than it declared via Content-Length.
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
    let mut body_prefix = Vec::new();
    loop {
        let mut buf = [0u8; 8192];
        let n = stream.read(&mut buf)?;
        if n == 0 {
            break;
        }
        head_bytes.extend_from_slice(&buf[..n]);
        if let Some(idx) = head_end(&head_bytes) {
            body_prefix = head_bytes.split_off(idx);
            break;
        }
        if head_bytes.len() > MAX_HEAD_BYTES {
            send_response(
                &mut stream,
                431,
                "Request Header Fields Too Large",
                "Request head too large (no blank line)",
            )?;
            return Ok(());
        }
    }
    if head_end(&head_bytes).is_none() {
        send_response(&mut stream, 400, "Bad Request", "Incomplete request head")?;
        return Ok(());
    }

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

    // Body uploads require an unambiguous Content-Length; anything else
    // (duplicate with differing values, non-numeric, absurdly large) gets a
    // reject rather than an over-read or a worker thread parked on a liar.
    let declared = content_length(&head_str);
    let is_write =
        (method == "POST" || method == "PUT") && (path == "/push" || path.starts_with("/artifact"));
    let body = if is_write {
        // Authenticate before trusting any body bytes: an anonymous client
        // must not park a worker thread on a huge declared body, force us to
        // read data that will be discarded, or probe storage with forged
        // writes. Unauthenticated mutations get a 401 without a byte read.
        if !require_write_auth(&mut stream, &head_str, token.as_deref())? {
            return Ok(());
        }
        let Some(len) = declared else {
            send_response(
                &mut stream,
                400,
                "Bad Request",
                "Content-Length missing or ambiguous",
            )?;
            return Ok(());
        };
        if len as u64 > MAX_BODY_BYTES {
            send_response(
                &mut stream,
                413,
                "Payload Too Large",
                "Request body too large",
            )?;
            return Ok(());
        }
        let mut body = body_prefix;
        let remaining = len.saturating_sub(body.len());
        if remaining > 0 {
            let mut rest = vec![0u8; remaining];
            stream.read_exact(&mut rest)?;
            body.extend_from_slice(&rest);
        }
        body
    } else {
        body_prefix
    };

    let (path, query) = split_path_query(path);

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
        ("POST", "/push") => handle_push(&mut stream, reg, verbose, &body)?,
        ("POST", "/artifact") => handle_artifact_push(&mut stream, reg, verbose, &body, query)?,
        _ => send_response(&mut stream, 404, "Not Found", "Endpoint not found")?,
    }

    Ok(())
}

/// require_write_auth - reject mutating requests unless a server token is
/// either unset or matched by an `Authorization: Bearer <token>` header.
///
/// Return: whether to continue handling the request
fn require_write_auth(stream: &mut TcpStream, head: &str, token: Option<&str>) -> Result<bool> {
    let Some(expect) = token else {
        return Ok(true);
    };
    let authorized = head.lines().any(|line| {
        let line = line.trim();
        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        let value = value.trim();
        name.trim().eq_ignore_ascii_case("authorization")
            && value.eq_ignore_ascii_case(&format!("Bearer {expect}"))
    });
    if authorized {
        return Ok(true);
    }
    send_response(
        stream,
        401,
        "Unauthorized",
        "Missing or invalid bearer token",
    )?;
    Ok(false)
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
/// Spec path format: `/artifact/<name>:<tag>`. Naming uses the same strict
/// validator as push, so anything publishable is fetchable. Responds with the
/// raw binary bytes (content type application/octet-stream) or 404 if absent.
///
/// Return: nothing
fn handle_artifact_get(
    stream: &mut TcpStream,
    reg: &LayerRegistry,
    spec: &str,
    verbose: bool,
) -> Result<()> {
    let (name, tag) = match split_name_tag(spec) {
        Ok(spec) => spec,
        Err(e) => {
            send_response(
                stream,
                400,
                "Bad Request",
                &format!("Invalid name:tag: {e}"),
            )?;
            return Ok(());
        }
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
/// Query params: `?name=<name>&tag=<tag>` (tag defaults to "latest").
/// Names are validated with the same strict alias check as the GET route so
/// every stored alias can be fetched back. The request body is the full
/// runnable binary; stores it content-addressed and returns the content hash.
///
/// Return: nothing
fn handle_artifact_push(
    stream: &mut TcpStream,
    reg: &mut LayerRegistry,
    verbose: bool,
    body: &[u8],
    query: &str,
) -> Result<()> {
    let params: std::collections::HashMap<String, String> =
        parse_query_params(query).into_iter().collect();
    let Some(name) = params.get("name").filter(|n| !n.is_empty()) else {
        send_response(stream, 400, "Bad Request", "Missing ?name= parameter")?;
        return Ok(());
    };
    let tag = params
        .get("tag")
        .filter(|t| !t.is_empty())
        .map(String::as_str)
        .unwrap_or("latest");

    if let Err(e) = split_name_tag(&format!("{name}:{tag}")) {
        send_response(
            stream,
            400,
            "Bad Request",
            &format!("Invalid name:tag: {e}"),
        )?;
        return Ok(());
    }

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
