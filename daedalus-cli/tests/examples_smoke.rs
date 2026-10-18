//! Non-hello example smokes that share one stub build (OnceLock in common).
//! Keep HTTP + adjacent-mode checks here so `cargo test` does not rebuild
//! the stub twice across separate test binaries.

#![cfg(unix)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

mod common;
use common::locate_stub;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn wait_http_ok(url_host_port: &str, needle: &str, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Ok(mut stream) = TcpStream::connect(url_host_port) {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
            let req = b"GET / HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
            if stream.write_all(req).is_ok() {
                let mut buf = String::new();
                if stream.read_to_string(&mut buf).is_ok() && buf.contains(needle) {
                    return true;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn build_example(app: &Path, out: &Path, stub: &Path, data: &Path, trust: &Path) {
    let cli = env!("CARGO_BIN_EXE_daedalus");
    let build = Command::new(cli)
        .args([
            "build",
            app.to_str().unwrap(),
            "--isolation",
            "none",
            "-o",
            out.to_str().unwrap(),
        ])
        .env("DAEDALUS_STUB_PATH", stub)
        .env("XDG_DATA_HOME", data)
        .env("DAEDALUS_TRUSTED_DIR", trust)
        .output()
        .expect("spawn build");
    assert!(
        build.status.success(),
        "build {}: {}",
        app.display(),
        String::from_utf8_lossy(&build.stderr)
    );
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(out, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn bottle_web_with_vendored_deps_builds_and_serves() {
    let Some(stub) = locate_stub() else {
        eprintln!("skipping: no daedalus-stub available");
        return;
    };

    let root = workspace_root();
    let app = root.join("examples/bottle-web");
    assert!(app.join("site-packages/bottle.py").is_file());

    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("bottle-web.de");
    let data = tmp.path().join("data");
    let trust = tmp.path().join("trusted");
    let cache = tmp.path().join("cache");
    let home = tmp.path().join("home");
    build_example(&app, &out, &stub, &data, &trust);

    // Prove the builder wired PYTHONPATH without needing a full HTTP round-trip
    // when the server is slow to bind (still serve below as the real smoke).
    let inspect = Command::new(env!("CARGO_BIN_EXE_daedalus"))
        .args(["inspect", out.to_str().unwrap(), "--json"])
        .output()
        .expect("inspect");
    let body = String::from_utf8_lossy(&inspect.stdout);
    assert!(
        body.contains("site-packages") || body.contains("PYTHONPATH"),
        "inspect must surface PYTHONPATH/site-packages: {body}"
    );

    let port = 18080u16;
    let mut child = Command::new(&out)
        .env("PORT", port.to_string())
        .env("XDG_CACHE_HOME", &cache)
        .env("XDG_DATA_HOME", &data)
        .env("HOME", &home)
        .env("DAEDALUS_TRUSTED_DIR", &trust)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn bottle-web.de");

    let ok = wait_http_ok(
        &format!("127.0.0.1:{port}"),
        "bottle",
        Duration::from_secs(20),
    );
    if !ok {
        stop(&mut child);
        let mut err = String::new();
        if let Some(mut e) = child.stderr.take() {
            let _ = e.read_to_string(&mut err);
        }
        panic!("bottle-web.de did not serve HTTP with vendored bottle: {err}");
    }
    stop(&mut child);
}

#[test]
fn field_kit_adjacent_modes_run_offline() {
    let Some(stub) = locate_stub() else {
        eprintln!("skipping: no daedalus-stub available");
        return;
    };

    let app = workspace_root().join("examples/field-kit");
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("field-kit.de");
    let data = tmp.path().join("data");
    let trust = tmp.path().join("trusted");
    let cache = tmp.path().join("cache");
    let home = tmp.path().join("home");
    build_example(&app, &out, &stub, &data, &trust);

    for (mode, needles) in [
        (
            "domains",
            &["usb-courier", "airgap-ops", "local-ai: only when weights"][..],
        ),
        ("handoff", &["daedalus trust", "daedalus verify"][..]),
        ("inventory", &["FIELD KIT: INVENTORY", "stdlib only"][..]),
        ("classroom", &["CLASSROOM"][..]),
        ("incident", &["INCIDENT", "private key OFF the USB"][..]),
        ("update", &["SISR", "apply chunks offline"][..]),
    ] {
        let run = Command::new(&out)
            .arg(mode)
            .env("XDG_CACHE_HOME", &cache)
            .env("XDG_DATA_HOME", &data)
            .env("HOME", &home)
            .env("DAEDALUS_TRUSTED_DIR", &trust)
            .output()
            .unwrap_or_else(|e| panic!("run {mode}: {e}"));
        assert!(
            run.status.success(),
            "mode {mode} failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
        let stdout = String::from_utf8_lossy(&run.stdout);
        for needle in needles {
            assert!(
                stdout.contains(needle),
                "mode {mode} missing {needle:?}: {stdout}"
            );
        }
    }
}
