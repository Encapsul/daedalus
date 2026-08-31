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
    eprintln!("[daedalus] registry server listening on {}", args.bind);
    eprintln!("[daedalus] storage: {}", dir.display());

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
/// Reads the first line of the request, extracts method and path, and routes
/// to handle_list, handle_pull, or handle_push.
///
/// Return: nothing
fn handle_connection(
    mut stream: TcpStream,
    reg: &mut LayerRegistry,
    token: Option<String>,
    verbose: bool,
) -> Result<()> {
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf)?;
    let request = String::from_utf8_lossy(&buf[..n]);

    let first_line = request.lines().next().unwrap_or("");
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

    match (method, path) {
        ("GET", "/list") => handle_list(&mut stream, reg)?,
        ("GET", path) if path.starts_with("/pull/") => {
            let hash = &path[6..];
            handle_pull(&mut stream, reg, hash, verbose)?;
        }
        ("POST", "/push") => handle_push(&mut stream, reg, token, verbose)?,
        _ => send_response(&mut stream, 404, "Not Found", "Endpoint not found")?,
    }

    Ok(())
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
) -> Result<()> {
    let body = extract_body(stream);
    if body.is_empty() {
        send_response(stream, 400, "Bad Request", "Empty body")?;
        return Ok(());
    }

    let layer: daedalus_core::layer::SerializableLayer = match serde_json::from_slice(&body) {
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

/// extract_body - read the HTTP request body from a TcpStream.
/// @stream: stream
///
/// Description:
/// Reads up to 4096 bytes from the stream and returns them as a Vec<u8>.
///
/// Return: vector of Vec<u8>
fn extract_body(stream: &mut TcpStream) -> Vec<u8> {
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).unwrap_or(0);
    buf[..n].to_vec()
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
