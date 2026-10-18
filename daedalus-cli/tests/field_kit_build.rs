//! Field-kit: adjacent-domain demo (USB hand-off, inventory, classroom).
//! Builds the example and asserts each mode's stdout marker.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

mod common;
use common::locate_stub;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn run_mode(
    bin: &Path,
    mode: &str,
    cache: &Path,
    data: &Path,
    home: &Path,
    trust: &Path,
) -> String {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = Command::new(bin)
        .arg(mode)
        .env("XDG_CACHE_HOME", cache)
        .env("XDG_DATA_HOME", data)
        .env("HOME", home)
        .env("DAEDALUS_TRUSTED_DIR", trust)
        .output()
        .unwrap_or_else(|e| panic!("run {mode}: {e}"));
    assert!(
        out.status.success(),
        "mode {mode} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
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
        .env("DAEDALUS_STUB_PATH", &stub)
        .env("XDG_DATA_HOME", &data)
        .env("DAEDALUS_TRUSTED_DIR", &trust)
        .output()
        .expect("spawn build");
    assert!(
        build.status.success(),
        "field-kit build: {}",
        String::from_utf8_lossy(&build.stderr)
    );

    let domains = run_mode(&out, "domains", &cache, &data, &home, &trust);
    assert!(domains.contains("usb-courier"));
    assert!(domains.contains("airgap-ops"));
    assert!(domains.contains("local-ai: only when weights"));

    let handoff = run_mode(&out, "handoff", &cache, &data, &home, &trust);
    assert!(handoff.contains("daedalus trust"));
    assert!(handoff.contains("daedalus verify"));

    let inventory = run_mode(&out, "inventory", &cache, &data, &home, &trust);
    assert!(inventory.contains("FIELD KIT: INVENTORY"));
    assert!(inventory.contains("stdlib only"));

    let classroom = run_mode(&out, "classroom", &cache, &data, &home, &trust);
    assert!(classroom.contains("CLASSROOM"));

    let incident = run_mode(&out, "incident", &cache, &data, &home, &trust);
    assert!(incident.contains("INCIDENT"));
    assert!(incident.contains("private key OFF the USB"));
}
