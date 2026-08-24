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

pub fn run(args: ServeArgs) -> Result<()> {
    match args.command {
        ServeCommand::Start(sub) => run_start(sub),
    }
}

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

fn handle_list(stream: &mut TcpStream, reg: &LayerRegistry) -> Result<()> {
    let layers = reg.list_layers().unwrap_or_default();
    let body = layers.join("\n");
    send_response(stream, 200, "OK", &body)
}

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

fn extract_body(stream: &mut TcpStream) -> Vec<u8> {
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).unwrap_or(0);
    buf[..n].to_vec()
}

fn send_response(stream: &mut TcpStream, code: u16, status: &str, body: &str) -> Result<()> {
    send_response_bytes(stream, code, status, body.as_bytes(), "text/plain")
}

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

fn expand_tilde(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    path.to_path_buf()
}
