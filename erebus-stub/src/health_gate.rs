//! Health check HTTP server for the erebus launcher stub.
//!
//! Provides `spawn_health_server` (daemon TCP thread) and
//! `maybe_start_health` (start if configured in metadata).

use std::io::{self, Read, Write};
use std::net::TcpListener;
use std::thread;

use crate::Metadata;

/// Spawn a daemon TCP thread that responds to health check requests.
/// Runs until the process exits (thread is detached).
pub fn spawn_health_server(port: u16, endpoint: String) {
    let listener = match TcpListener::bind(format!("127.0.0.1:{port}")) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[erebus] warning: health check server failed to bind on port {port}: {e}");
            return;
        }
    };
    let _ = listener.set_nonblocking(true);

    thread::spawn(move || {
        let response_200 = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 14\r\nConnection: close\r\n\r\n{\"status\":\"ok\"}";
        let response_404 =
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buf = [0u8; 1024];
                    let n = stream.read(&mut buf).unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..n]);
                    let first_line = request.lines().next().unwrap_or("");
                    if first_line.starts_with("GET") && first_line.contains(endpoint.as_str()) {
                        let _ = stream.write_all(response_200);
                    } else {
                        let _ = stream.write_all(response_404);
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(_) => {
                    thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }
    });
}

/// Start the health check server if configured in metadata.
pub fn maybe_start_health(meta: &Metadata) {
    if let Some(ref hc) = meta.health_check {
        if hc.enabled && hc.port > 0 {
            spawn_health_server(hc.port, hc.endpoint.clone());
        }
    }
}
