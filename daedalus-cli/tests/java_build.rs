//! Phase 8 Step 3 end-to-end: a real Gradle project is detected, packaged by
//! `gradle build`, the JAR is staged into the rootfs, and the assembled
//! binary must run and print the app's output.
//!
//! Requires `java` and `gradle` on PATH (checked, not downloaded). The JRE
//! is embedded via `--embed-interpreter java` because the stub only execs
//! interpreters found inside the rootfs. Skips with an explicit message
//! when a prerequisite is missing.

use std::path::{Path, PathBuf};
use std::process::Command;

fn tool_available(tool: &str) -> bool {
    Command::new(tool)
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn gradle_version_ok() -> bool {
    // Gradle 4.x (Ubuntu default) is incompatible with JDK 17+; need ≥ 7.
    // In CI we install a modern gradle; here we skip if unavailable.
    let output = Command::new("gradle").arg("-version").output();
    if let Ok(output) = output {
        if let Ok(stdout) = String::from_utf8(output.stdout) {
            return stdout
                .lines()
                .find(|l| l.contains("Gradle"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<u32>().ok())
                .map(|major| major >= 7)
                .unwrap_or(false);
        }
    }
    false
}

/// The workspace target dir, resolved independently of the test's CWD.
fn workspace_target_dir() -> PathBuf {
    std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("manifest dir has a parent")
                .join("target")
        })
}

/// Locates (or builds) a runnable stub for the host. Returns `None` when no
/// stub can be produced — the caller skips instead of failing.
///
/// A plain `cargo build -p daedalus-stub` runs on every call: it is a fast
/// no-op when fresh, which GUARANTEES the embedded stub matches the current
/// sources — a stale prebuilt stub fails with confusing errors at exec time.
fn locate_stub() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("DAEDALUS_STUB_PATH") {
        let p = PathBuf::from(path);
        if p.is_file() {
            return Some(p);
        }
    }
    let status = Command::new("cargo")
        .args(["build", "-q", "-p", "daedalus-stub"])
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    let stub = workspace_target_dir().join("debug/daedalus-stub");
    stub.is_file().then_some(stub)
}

fn write_hello_project(dir: &Path) {
    std::fs::create_dir_all(dir.join("src/main/java/hello")).unwrap();
    std::fs::write(
        dir.join("build.gradle"),
        "apply plugin: 'java'\n\
         version = '1.0'\n\
         jar {\n\
             manifest {\n\
                 attributes 'Main-Class': 'hello.Hello'\n\
             }\n\
         }\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main/java/hello/Hello.java"),
        "package hello;\n\
         public class Hello {\n\
             public static void main(String[] args) {\n\
                 System.out.println(\"hello-from-daedalus-java\");\n\
             }\n\
         }\n",
    )
    .unwrap();
}

#[test]
fn java_app_is_detected_built_and_runs() {
    if !tool_available("java") || !gradle_version_ok() {
        eprintln!("skipping: java/gradle ≥ 7 not available (system gradle is too old for JDK 17+)");
        return;
    }
    let Some(stub) = locate_stub() else {
        eprintln!("skipping: no daedalus-stub available to assemble with");
        return;
    };
    eprintln!("using stub: {}", stub.display());

    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("hello-java");
    write_hello_project(&project);

    let out = tmp.path().join("hello.daedalus");
    let cli = env!("CARGO_BIN_EXE_daedalus");
    let build = Command::new(cli)
        .args([
            "build",
            project.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            // No JRE in the rootfs otherwise — the stub refuses to exec.
            "--embed-interpreter",
            "java",
        ])
        // NOTE: no `--no-install` — it also skips the gradle build itself,
        // and this test must exercise it.
        .env("DAEDALUS_STUB_PATH", &stub)
        .output()
        .expect("failed to spawn daedalus build");
    let stderr = String::from_utf8_lossy(&build.stderr).into_owned();
    assert!(
        build.status.success(),
        "daedalus build must succeed for a Java app: {stderr}"
    );
    assert!(out.is_file(), "artifact must exist: {stderr}");

    // The assembled artifact is self-extracting: running it must extract the
    // rootfs (JRE included) and exec `java -jar /app/hello-1.0.jar`.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let run = Command::new(&out)
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .env("XDG_DATA_HOME", tmp.path().join("data"))
        .env("HOME", tmp.path().join("home"))
        .output()
        .expect("failed to run the assembled Java artifact");
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    assert!(
        run.status.success(),
        "assembled binary must exit 0: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        stdout.contains("hello-from-daedalus-java"),
        "assembled binary must print the Java app output, got: {stdout}"
    );
}
