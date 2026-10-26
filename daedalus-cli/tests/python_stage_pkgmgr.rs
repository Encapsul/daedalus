//! uv / Poetry staging: export → `pip install --target` into site-packages.
//! Uses PATH shims so CI does not need real uv/poetry installs.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;
use common::locate_stub;

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn copy_fixture(name: &str, dest: &Path) {
    let src = fixtures_root().join(name);
    copy_dir(&src, dest);
}

fn copy_dir(src: &Path, dest: &Path) {
    fs::create_dir_all(dest).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dest.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &to);
        } else {
            fs::copy(entry.path(), &to).unwrap();
        }
    }
}

/// Fake `uv`/`poetry` that honors `-o <path>` and writes a local **wheel**.
///
/// Path deps (`./local_dep` + setup.py) force pip to fetch setuptools from
/// PyPI under build isolation — that fails on offline macOS CI. A purelib
/// wheel installs with `PIP_NO_INDEX=1`.
fn write_export_shim(bin_dir: &Path, tool: &str) {
    fs::create_dir_all(bin_dir).unwrap();
    let path = bin_dir.join(tool);
    let script = r#"#!/bin/sh
out=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o|--output) out="$2"; shift 2 ;;
    *) shift ;;
  esac
done
if [ -z "$out" ]; then
  echo "shim: missing -o" >&2
  exit 1
fi
printf '%s\n' "./local_dep/markpkg-0.0.1-py3-none-any.whl" > "$out"
"#;
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn path_with_shim(shim_bin: &Path) -> String {
    let old = std::env::var("PATH").unwrap_or_default();
    format!("{}:{}", shim_bin.display(), old)
}

/// Drop directories that contain `tool` so a missing-tool failure is reliable.
fn path_without_tool(tool: &str) -> String {
    std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .filter(|p| !p.is_empty() && !Path::new(p).join(tool).is_file())
        .collect::<Vec<_>>()
        .join(":")
}

fn build_with_env(
    app: &Path,
    out: &Path,
    stub: &Path,
    data: &Path,
    trust: &Path,
    path: &str,
) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_daedalus"))
        .args([
            "build",
            app.to_str().unwrap(),
            "--isolation",
            "none",
            "-o",
            out.to_str().unwrap(),
            "-v",
        ])
        .env("PATH", path)
        .env("DAEDALUS_STUB_PATH", stub)
        .env("XDG_DATA_HOME", data)
        .env("DAEDALUS_TRUSTED_DIR", trust)
        // Staging tests must not touch PyPI (macOS runners have flaky DNS).
        .env("PIP_NO_INDEX", "1")
        .env("PIP_DISABLE_PIP_VERSION_CHECK", "1")
        .output()
        .expect("spawn build")
}

#[test]
fn uv_lock_stages_via_export_shim() {
    let Some(stub) = locate_stub() else {
        eprintln!("skipping: no daedalus-stub available");
        return;
    };
    let td = tempfile::tempdir().unwrap();
    let app = td.path().join("app");
    copy_fixture("python-uv", &app);
    assert_eq!(
        daedalus_core::pkgmgr::detect_python_pkgmgr(&app),
        Some(daedalus_core::pkgmgr::PkgMgr::Uv)
    );

    let shim = td.path().join("shim-bin");
    write_export_shim(&shim, "uv");
    let out = td.path().join("app.de");
    let data = td.path().join("data");
    let trust = td.path().join("trust");
    let output = build_with_env(&app, &out, &stub, &data, &trust, &path_with_shim(&shim));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "uv staged build failed: {stderr}");
    assert!(
        stderr.contains("uv export") || stderr.contains("pip install"),
        "expected staged export/pip log: {stderr}"
    );
    fs::set_permissions(&out, fs::Permissions::from_mode(0o755)).unwrap();

    let run = Command::new(&out)
        .env("XDG_CACHE_HOME", td.path().join("cache"))
        .env("HOME", td.path().join("home"))
        .env("DAEDALUS_TRUSTED_DIR", &trust)
        .output()
        .expect("run .de");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success() && stdout.contains("uv-staged"),
        "runtime missing staged markpkg: status={:?} stdout={stdout} stderr={}",
        run.status.code(),
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn poetry_lock_stages_via_export_shim() {
    let Some(stub) = locate_stub() else {
        eprintln!("skipping: no daedalus-stub available");
        return;
    };
    let td = tempfile::tempdir().unwrap();
    let app = td.path().join("app");
    copy_fixture("python-poetry", &app);
    assert_eq!(
        daedalus_core::pkgmgr::detect_python_pkgmgr(&app),
        Some(daedalus_core::pkgmgr::PkgMgr::Poetry)
    );

    let shim = td.path().join("shim-bin");
    write_export_shim(&shim, "poetry");
    let out = td.path().join("app.de");
    let data = td.path().join("data");
    let trust = td.path().join("trust");
    let output = build_with_env(&app, &out, &stub, &data, &trust, &path_with_shim(&shim));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "poetry staged build failed: {stderr}"
    );
    assert!(
        stderr.contains("poetry export") || stderr.contains("pip install"),
        "expected staged export/pip log: {stderr}"
    );
    fs::set_permissions(&out, fs::Permissions::from_mode(0o755)).unwrap();

    let run = Command::new(&out)
        .env("XDG_CACHE_HOME", td.path().join("cache"))
        .env("HOME", td.path().join("home"))
        .env("DAEDALUS_TRUSTED_DIR", &trust)
        .output()
        .expect("run .de");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success() && stdout.contains("poetry-staged"),
        "runtime missing staged markpkg: status={:?} stdout={stdout} stderr={}",
        run.status.code(),
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn uv_lock_without_uv_fails_clearly() {
    let Some(stub) = locate_stub() else {
        eprintln!("skipping: no daedalus-stub available");
        return;
    };
    let td = tempfile::tempdir().unwrap();
    let app = td.path().join("app");
    copy_fixture("python-uv", &app);

    let out = td.path().join("app.de");
    let data = td.path().join("data");
    let trust = td.path().join("trust");
    let output = build_with_env(&app, &out, &stub, &data, &trust, &path_without_tool("uv"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "expected failure, got: {stderr}");
    assert!(
        stderr.contains('`') && stderr.contains("uv") && stderr.contains("site-packages"),
        "expected clear missing-uv staging error: {stderr}"
    );
}
