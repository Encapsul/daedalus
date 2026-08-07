//! A std-only HTTP/1.1 mock server for the SISR E2E tests.
//!
//! Serves the content-addressed chunks and the `XBMR` manifest from an
//! in-memory route table, exactly like the update channel described in the
//! launcher docs (`{base}/manifest`, `{base}/chunks/<64-hex-sha256>`).
//!
//! No external dependency, no privileged port: `TcpListener` binds a random
//! high port on loopback, so the suite runs in CI without root.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// A minimal mock update channel.
///
/// Requests to unknown paths receive `404 Not Found`, mirroring the real
/// server behavior when a chunk file is missing.
#[derive(Clone)]
pub struct MockHttpServer {
    routes: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    shutdown: Arc<AtomicBool>,
}

impl MockHttpServer {
    /// Starts the server on a random loopback port and returns a handle plus
    /// the base URL (`http://127.0.0.1:<port>`).
    pub fn start() -> (Self, String) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("mock server bind failed");
        listener
            .set_nonblocking(true)
            .expect("mock server nonblocking failed");
        let port = listener.local_addr().expect("mock server address").port();

        let routes = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));

        let accept_routes = routes.clone();
        let accept_shutdown = shutdown.clone();
        thread::spawn(move || {
            while !accept_shutdown.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let routes = accept_routes.clone();
                        thread::spawn(move || serve(&mut stream, &routes));
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        let server = Self { routes, shutdown };
        let base_url = format!("http://127.0.0.1:{port}");
        (server, base_url)
    }

    /// Registers (or replaces) the response body served for an exact path.
    pub fn route(&self, path: &str, body: Vec<u8>) {
        self.routes
            .lock()
            .expect("mock server route lock")
            .insert(path.to_string(), body);
    }

    /// Serves the raw `XBMR` manifest at `{base}/manifest`.
    pub fn route_manifest(&self, manifest: &[u8]) {
        self.route("/manifest", manifest.to_vec());
    }
}

impl Drop for MockHttpServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

/// Serves one HTTP/1.1 GET request: reads the request head, looks up the path,
/// and replies with the body (or 404). `Connection: close` keeps the protocol
/// simple — the client reconnects per request.
fn serve(stream: &mut TcpStream, routes: &Arc<Mutex<HashMap<String, Vec<u8>>>>) {
    let path = match read_request_path(stream) {
        Some(p) => p,
        None => return,
    };
    let body = routes
        .lock()
        .expect("mock server routes lock")
        .get(&path)
        .cloned();
    let (status, body) = match body {
        Some(body) => ("200 OK", body),
        None => ("404 Not Found", b"chunk not found".to_vec()),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    if stream.write_all(head.as_bytes()).is_err() || stream.write_all(&body).is_err() {
        return;
    }
    let _ = stream.flush();
}

/// Reads the request head and returns the request target (path only).
fn read_request_path(stream: &mut TcpStream) -> Option<String> {
    let mut head = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => return None,
            Ok(n) => {
                head.extend_from_slice(&buf[..n]);
                if head.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
        }
    }
    let head = String::from_utf8_lossy(&head);
    head.lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .map(|target| target.split('?').next().unwrap_or(target).to_string())
}
