//! Process execution for the xbin launcher stub.
//!
//! Provides single-service exec, multi-service supervisor, entrypoint
//! resolution, environment setup, and platform-specific process spawning.

use std::collections::BTreeMap;
#[cfg(unix)]
use std::ffi::CString;
use std::io;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::exit;

use crate::config::AppConfig;
use crate::Metadata;
#[cfg(unix)]
use crate::{cstr, to_ptr_vec};

/// Enter user + mount namespace if isolation >= 2; no-op otherwise.
/// No-op on non-Linux platforms (no namespaces available).
#[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
pub fn enter_namespace_if_needed(isolation: u8) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    if isolation >= 2 {
        crate::namespace::enter_userns()?;
    }
    Ok(())
}

/// Build the process environment: host env + `LD_LIBRARY_PATH` + meta.env + `ROOTFS` substitution.
/// When `orig_cwd` is Some, inserts `XBIN_ORIG_CWD` (used by single-service exec).
pub fn setup_env(
    meta: &Metadata,
    rootfs: &Path,
    use_pivot: bool,
    orig_cwd: Option<&Path>,
    app_config: &AppConfig,
) -> io::Result<BTreeMap<String, String>> {
    let mut env: BTreeMap<String, String> = std::env::vars().collect();

    // LD_LIBRARY_PATH only matters for ELF dynamic linking; on macOS/Windows
    // the OS loader resolves system libs on its own.
    #[cfg(unix)]
    {
        if use_pivot {
            env.insert("LD_LIBRARY_PATH".into(), crate::LD_PATHS_ABS.join(":"));
        } else {
            let mut paths: Vec<String> = crate::LD_PATHS
                .iter()
                .map(|p| rootfs.join(p))
                .filter(|p| p.exists())
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            if let Some(existing) = env.get("LD_LIBRARY_PATH") {
                if !existing.is_empty() {
                    let existing_entries: Vec<String> = existing
                        .split(':')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    for entry in &existing_entries {
                        if !paths.iter().any(|p| p == entry) {
                            paths.push(entry.clone());
                        }
                    }
                }
            }
            env.insert("LD_LIBRARY_PATH".into(), paths.join(":"));
        }
    }

    // PATH: bundled binaries (usr/bin, bin, usr/local/bin) before system PATH.
    if use_pivot {
        env.insert("PATH".into(), crate::BIN_PATHS_ABS.join(":"));
    } else {
        let mut paths: Vec<String> = crate::BIN_PATHS
            .iter()
            .map(|p| rootfs.join(p))
            .filter(|p| p.exists())
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        if let Some(existing) = env.get("PATH") {
            if !existing.is_empty() {
                paths.push(existing.clone());
            }
        }
        let sep = if cfg!(windows) { ";" } else { ":" };
        env.insert("PATH".into(), paths.join(sep));
    }

    if let Some(cwd) = orig_cwd {
        env.insert("XBIN_ORIG_CWD".into(), cwd.to_string_lossy().into_owned());
    }

    let rootfs_str = rootfs.to_string_lossy();
    for (k, v) in &meta.env {
        env.insert(k.clone(), v.replace("${ROOTFS}", &rootfs_str));
    }

    // Merge secrets from config file
    if let Some(secrets) = &app_config.secrets {
        for (k, v) in secrets {
            env.insert(format!("XBIN_SECRET_{}", k.to_uppercase()), v.clone());
        }
    }

    // Merge database URL from config
    if let Some(url) = app_config.get_database_url() {
        env.insert("DATABASE_URL".into(), url);
    }

    let dotenv_env = xbin_core::dotenv::load_dotenv(rootfs, None, false);
    for (k, v) in dotenv_env {
        env.insert(k, v);
    }

    // Framework-specific defaults
    match meta.runtime.as_str() {
        "python" => {
            env.insert("PYTHONUNBUFFERED".into(), "1".into());
            if !env.contains_key("DJANGO_SETTINGS_MODULE") {
                if let Some(settings) = detect_django_settings(rootfs) {
                    env.insert("DJANGO_SETTINGS_MODULE".into(), settings);
                }
            }
        }
        "node" => {
            env.insert("NODE_ENV".into(), "production".into());
        }
        "php" => {
            if !env.contains_key("APP_ENV") {
                env.insert("APP_ENV".into(), "production".into());
            }
            if !env.contains_key("APP_DEBUG") {
                env.insert("APP_DEBUG".into(), "0".into());
            }
        }
        "ruby" => {
            env.insert("RAILS_ENV".into(), "production".into());
            env.insert("RACK_ENV".into(), "production".into());
        }
        "java" if !env.contains_key("JAVA_TOOL_OPTIONS") => {
            if let Ok(java_opts) = std::env::var("JAVA_OPTS").or_else(|_| std::env::var("JVM_OPTS"))
            {
                env.insert("JAVA_TOOL_OPTIONS".into(), java_opts);
            }
        }
        _ => {}
    }

    // Ensure PORT is set for web frameworks
    if !env.contains_key("PORT") {
        if let Some(port) = detect_web_port(rootfs, &meta.runtime) {
            env.insert("PORT".into(), port.to_string());
        }
    }

    // Add native library directories to LD_LIBRARY_PATH
    #[cfg(unix)]
    {
        use walkdir::WalkDir;
        let mut lib_dirs = Vec::new();
        for entry in WalkDir::new(rootfs).max_depth(3).into_iter().flatten() {
            let p = entry.path();
            if p.is_file() {
                if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                    let is_so = ext == "so"
                        || p.file_name()
                            .map(|n| n.to_string_lossy().contains(".so."))
                            .unwrap_or(false);
                    if is_so || ext == "dylib" {
                        if let Some(parent) = p.parent() {
                            if parent != rootfs {
                                lib_dirs.push(parent.to_string_lossy().into_owned());
                            }
                        }
                    }
                }
            }
        }
        lib_dirs.sort();
        lib_dirs.dedup();
        if !lib_dirs.is_empty() {
            let existing = env.get("LD_LIBRARY_PATH").map(String::as_str).unwrap_or("");
            let mut paths: Vec<String> = lib_dirs;
            if !existing.is_empty() {
                for entry in existing.split(':') {
                    if !entry.is_empty() && !paths.contains(&entry.to_string()) {
                        paths.push(entry.to_string());
                    }
                }
            }
            env.insert("LD_LIBRARY_PATH".into(), paths.join(":"));
        }
    }

    // Ensure writable directories exist for apps that need them (DB, temp files)
    for dir in &["/app/data", "/app/tmp"] {
        let path = if use_pivot {
            PathBuf::from(dir)
        } else {
            rootfs.join(dir.strip_prefix('/').unwrap_or(dir))
        };
        let _ = std::fs::create_dir_all(&path);
    }

    Ok(env)
}

/// Detect Django settings module from manage.py or settings.py layout.
pub fn detect_django_settings(app_dir: &Path) -> Option<String> {
    if let Ok(content) = std::fs::read_to_string(app_dir.join("manage.py")) {
        if let Some(line) = content
            .lines()
            .find(|l| l.contains("DJANGO_SETTINGS_MODULE"))
        {
            if let Some(module) = line.split("DJANGO_SETTINGS_MODULE").nth(1) {
                let module = module.trim();
                // Use rfind to locate the VALUE's closing quote. After splitting
                // on "DJANGO_SETTINGS_MODULE", the first quote in the remainder
                // is often the closing quote of the key argument (e.g. `setdefault("KEY", "value")`).
                // We want the last pair of matching quotes — that's the value.
                if let Some(close) = module.rfind('"') {
                    if let Some(open) = module[..close].rfind('"') {
                        return Some(module[open + 1..close].to_string());
                    }
                }
                if let Some(close) = module.rfind('\'') {
                    if let Some(open) = module[..close].rfind('\'') {
                        return Some(module[open + 1..close].to_string());
                    }
                }
                let module = module
                    .trim_start_matches(|c: char| c == '=' || c.is_whitespace())
                    .trim_end_matches(|c: char| c == ')' || c == ',' || c.is_whitespace());
                if !module.is_empty() {
                    return Some(module.to_string());
                }
            }
        }
    }
    // Fallback: look for settings.py in a package directory
    if let Ok(entries) = std::fs::read_dir(app_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() && p.join("__init__.py").is_file() && p.join("settings.py").is_file() {
                let pkg = p.file_name()?.to_str()?;
                return Some(format!("{pkg}.settings"));
            }
        }
    }
    None
}

/// Detect default web port from app configuration files.
pub fn detect_web_port(app_dir: &Path, runtime: &str) -> Option<u16> {
    match runtime {
        "python" => {
            if app_dir.join("manage.py").is_file() {
                return Some(8000);
            }
            if let Ok(content) = std::fs::read_to_string(app_dir.join("requirements.txt")) {
                if content.contains("streamlit") {
                    return Some(8501);
                }
                if content.contains("gradio") {
                    return Some(7860);
                }
            }
        }
        "php" => {
            if app_dir.join("artisan").is_file() {
                return Some(8000);
            }
            return Some(8080);
        }
        "node" => {
            if let Ok(content) = std::fs::read_to_string(app_dir.join("package.json")) {
                if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(port) = pkg
                        .get("scripts")
                        .and_then(|s| s.get("start"))
                        .and_then(|v| v.as_str())
                        .and_then(|cmd| {
                            if let Some(port_str) = cmd.split("--port").nth(1) {
                                let port_str = port_str.split_whitespace().next()?;
                                let port_str = port_str.strip_prefix('=').unwrap_or(port_str);
                                port_str.parse::<u16>().ok()
                            } else {
                                None
                            }
                        })
                    {
                        return Some(port);
                    }
                }
            }
        }
        "ruby" if app_dir.join("bin").join("rails").is_file() => return Some(3000),
        _ => {}
    }
    None
}

/// Resolve a rootfs path: absolute if using `pivot_root`, relative to rootfs otherwise.
pub fn make_resolve<'a>(rootfs: &'a Path, use_pivot: bool) -> impl Fn(&str) -> PathBuf + 'a {
    move |p: &str| -> PathBuf {
        if use_pivot {
            PathBuf::from(p)
        } else if let Some(stripped) = p.strip_prefix('/') {
            rootfs.join(stripped)
        } else {
            PathBuf::from(p)
        }
    }
}

// ---------------------------------------------------------------------------
// Unix exec helpers
// ---------------------------------------------------------------------------

/// Convert a `BTreeMap<String,String>` to a null-terminated `Vec<CString>` for execve.
#[cfg(unix)]
pub fn env_to_cstrings(env: &BTreeMap<String, String>) -> io::Result<Vec<CString>> {
    env.iter()
        .map(|(k, v)| cstr(format!("{k}={v}").as_bytes()))
        .collect()
}

/// Check if an executable path exists and is executable.
/// Searches PATH directories when given a bare name (no directory component).
#[cfg(unix)]
pub fn is_executable(prog: &[u8]) -> bool {
    if prog.is_empty() {
        return false;
    }
    let path = String::from_utf8_lossy(prog);
    // If it's an absolute or relative path with a directory component, check it directly.
    if path.contains('/') || path.contains('\\') {
        return check_executable(&path);
    }
    // Otherwise search PATH directories (mirrors execvp behavior).
    let paths = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(path.as_ref());
        if check_executable(&candidate.to_string_lossy()) {
            return true;
        }
    }
    false
}

/// Check if a specific path points to an executable file.
pub fn check_executable(path: &str) -> bool {
    std::fs::metadata(path).is_ok_and(|m| {
        m.is_file() && {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                m.permissions().mode() & 0o111 != 0
            }
            #[cfg(windows)]
            {
                true
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Single-service exec
// ---------------------------------------------------------------------------

/// Launch the app entrypoint. Blocks until the app exits (or never returns on
/// successful execvp).
pub fn exec_app(meta: &Metadata, rootfs: &Path, app_config: &AppConfig) -> io::Result<()> {
    if meta.entrypoint.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "empty entrypoint",
        ));
    }

    #[cfg(unix)]
    let orig_cwd = std::env::current_dir().ok();
    #[cfg(target_os = "linux")]
    let use_pivot = meta.isolation >= 2;
    #[cfg(all(unix, not(target_os = "linux")))]
    let use_pivot = false;

    crate::health_gate::maybe_start_health(meta);

    #[cfg(target_os = "linux")]
    if use_pivot && crate::namespace::running_in_container() {
        eprintln!("[xbin] warning: running inside a container — namespace isolation may be restricted by the host");
    }

    enter_namespace_if_needed(meta.isolation)?;
    #[cfg(target_os = "linux")]
    if use_pivot {
        crate::pivot_root_into(rootfs)?;
        if meta.seccomp {
            if let Err(e) = crate::seccomp::install_seccomp_denylist() {
                eprintln!(
                    "[xbin] warning: seccomp not available, running without syscall filter: {e}"
                );
            }
        }
        if meta.landlock {
            if let Err(e) = crate::landlock::sandbox(rootfs) {
                eprintln!(
                    "[xbin] warning: landlock not available, running without filesystem sandbox: {e}"
                );
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        if meta.landlock {
            crate::macos_sandbox::apply_sandbox(rootfs);
        }
    }

    // ── Platform-specific argv build + process launch ─────────────────────
    #[cfg(unix)]
    {
        let (prog, direct_exec, interpreter_name) = resolve_entrypoint(meta, rootfs, use_pivot);
        let resolve = make_resolve(rootfs, use_pivot);
        let prog_c = cstr(prog.as_os_str().as_bytes())?;
        let prog_path_bytes = prog.as_os_str().as_bytes();

        if !is_executable(prog_path_bytes) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "[xbin] error: interpreter '{}' not found (tried: {})",
                    interpreter_name,
                    prog.display()
                ),
            ));
        }

        let mut argv: Vec<CString> = Vec::new();
        if direct_exec {
            argv.push(prog_c.clone());
            for a in &meta.entrypoint[1..] {
                argv.push(cstr(resolve(a).as_os_str().as_bytes())?);
            }
        } else {
            if !is_executable(interpreter_name.as_bytes()) {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("[xbin] error: interpreter '{}' not found", interpreter_name),
                ));
            }
            argv.push(cstr(interpreter_name.as_bytes())?);
            for a in &meta.entrypoint[1..] {
                argv.push(cstr(resolve(a).as_os_str().as_bytes())?);
            }
        }
        for a in std::env::args_os().skip(1) {
            argv.push(cstr(a.as_bytes())?);
        }

        let env = setup_env(meta, rootfs, use_pivot, orig_cwd.as_deref(), app_config)?;

        if let Some(cwd) = &meta.cwd {
            let dir = resolve(cwd);
            std::env::set_current_dir(&dir).ok();
        }
        for (k, v) in &env {
            std::env::set_var(k, v);
        }

        let argv_ptrs = to_ptr_vec(&argv);
        // SAFETY: execvp(3) replaces the current process. prog_c is a valid
        // CString, argv_ptrs is null-terminated. We never return on success.
        // execvp searches PATH for bare command names (e.g. "python3") and
        // uses absolute paths as-is (e.g. "/app/app.py"). Environment is
        // inherited from the current process after set_var calls above.
        unsafe {
            crate::libc_execvp(prog_c.as_ptr(), argv_ptrs.as_ptr());
        }
        Err(io::Error::last_os_error())
    }

    #[cfg(windows)]
    {
        let child = spawn_app_windows(meta, rootfs, app_config)?;
        let code = crate::win::wait(&child)?;
        exit(code);
    }
}

/// Resolve the entrypoint program + interpreter details shared by the
/// single-app and service-supervisor launch paths.
pub fn resolve_entrypoint(
    meta: &Metadata,
    rootfs: &Path,
    use_pivot: bool,
) -> (PathBuf, bool, String) {
    let resolve = make_resolve(rootfs, use_pivot);
    let mut prog = resolve(&meta.entrypoint[0]);

    // Compiled binaries (go/binary) exec `entrypoint[0]` directly; interpreted
    // runtimes get their interpreter prepended to argv.
    let direct_exec = matches!(meta.runtime.as_str(), "go" | "binary");
    let interpreter_name = match meta.runtime.as_str() {
        "php" => "php",
        "python" => "python3",
        "node" => "node",
        "ruby" => "ruby",
        "perl" => "perl",
        "java" => "java",
        "deno" => "deno",
        _ => "bash",
    };

    // For bare interpreter names without pivot_root, search rootfs bin dirs
    // so embedded interpreters are found before namespace/pivot setup.
    if !use_pivot && !meta.entrypoint[0].contains('/') {
        let prog_str = prog.to_string_lossy();
        if !check_executable(&prog_str) {
            if let Some(found) = crate::BIN_PATHS.iter().find_map(|dir| {
                let candidate = rootfs.join(dir).join(&meta.entrypoint[0]);
                if check_executable(&candidate.to_string_lossy()) {
                    Some(candidate)
                } else {
                    None
                }
            }) {
                prog = found;
            }
        }
    }

    (prog, direct_exec, interpreter_name.to_string())
}

// ---------------------------------------------------------------------------
// Windows exec helpers
// ---------------------------------------------------------------------------

/// Spawn the app as a child process on Windows (`CreateProcess`), returning the
/// child handle. The caller decides whether to poll (health gate) or wait.
#[cfg(windows)]
pub fn spawn_app_windows(
    meta: &Metadata,
    rootfs: &Path,
    app_config: &AppConfig,
) -> io::Result<crate::win::Child> {
    let (prog, direct_exec, interpreter_name) = resolve_entrypoint(meta, rootfs, false);
    let env = setup_env(meta, rootfs, false, None, app_config)?;
    let cwd = meta.cwd.as_ref().map(|c| make_resolve(rootfs, false)(c));
    let resolve = make_resolve(rootfs, false);

    let mut cmd = prog;
    if !is_executable_path(&cmd) {
        cmd = find_in_bin_paths(rootfs, &meta.entrypoint[0]).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "[xbin] error: interpreter '{}' not found (tried: {})",
                    interpreter_name,
                    cmd.display()
                ),
            )
        })?;
    }

    let mut argv: Vec<std::ffi::OsString> = Vec::new();
    if direct_exec {
        argv.push(cmd.as_os_str().to_os_string());
    } else {
        argv.push(std::ffi::OsString::from(interpreter_name));
    }
    for a in &meta.entrypoint[1..] {
        argv.push(resolve(a).into_os_string());
    }
    for a in std::env::args_os().skip(1) {
        argv.push(a);
    }

    crate::win::spawn(&cmd, &argv, &env, cwd.as_deref(), false)
}

/// Resolve a bare interpreter/command name against the rootfs bin dirs,
/// trying the `.exe` suffix on Windows.
#[cfg(windows)]
pub fn find_in_bin_paths(rootfs: &Path, name: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    let candidates = [name.to_string(), format!("{name}.exe")];
    #[cfg(not(windows))]
    let candidates = [name.to_string()];
    crate::BIN_PATHS.iter().find_map(|dir| {
        let base = rootfs.join(dir);
        candidates.iter().find_map(|c| {
            let candidate = base.join(c);
            if check_executable(&candidate.to_string_lossy()) {
                Some(candidate)
            } else {
                None
            }
        })
    })
}

/// `is_executable` for a `Path` (avoids unix-only `OsStr::as_bytes`).
#[cfg(windows)]
pub fn is_executable_path(path: &Path) -> bool {
    #[cfg(unix)]
    {
        is_executable(path.as_os_str().as_bytes())
    }
    #[cfg(not(unix))]
    {
        check_executable(&path.to_string_lossy())
    }
}

// ---------------------------------------------------------------------------
// Multi-process supervisor
// ---------------------------------------------------------------------------

/// Supervise multiple services: fork+exec each, health-check ports, wait for all.
#[cfg(unix)]
pub fn supervise_services(
    meta: &Metadata,
    rootfs: &Path,
    app_config: &AppConfig,
) -> io::Result<()> {
    let verbose = std::env::var_os("XBIN_VERBOSE").is_some();
    #[cfg(target_os = "linux")]
    let use_pivot = meta.isolation >= 2;
    #[cfg(not(target_os = "linux"))]
    let use_pivot = false;

    crate::health_gate::maybe_start_health(meta);

    enter_namespace_if_needed(meta.isolation)?;
    #[cfg(target_os = "linux")]
    if use_pivot {
        crate::pivot_root_into(rootfs)?;
        if meta.seccomp {
            if let Err(e) = crate::seccomp::install_seccomp_denylist() {
                eprintln!(
                    "[xbin] warning: seccomp not available, running without syscall filter: {e}"
                );
            }
        }
        if meta.landlock {
            if let Err(e) = crate::landlock::sandbox(rootfs) {
                eprintln!(
                    "[xbin] warning: landlock not available, running without filesystem sandbox: {e}"
                );
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        if meta.landlock {
            crate::macos_sandbox::apply_sandbox(rootfs);
        }
    }

    let base_env = setup_env(meta, rootfs, use_pivot, None, app_config)?;
    let resolve = make_resolve(rootfs, use_pivot);

    let children = fork_services(meta, &base_env, &resolve, rootfs, verbose)?;
    wait_for_health(meta, verbose)?;
    install_signal_handler(&children);
    wait_for_children(&children, verbose)
}

/// Supervise multiple services on Windows: spawn each with `CreateProcess`,
/// health-check ports, then wait for all handles.
#[cfg(windows)]
pub fn supervise_services(
    meta: &Metadata,
    rootfs: &Path,
    app_config: &AppConfig,
) -> io::Result<()> {
    let verbose = std::env::var_os("XBIN_VERBOSE").is_some();

    crate::health_gate::maybe_start_health(meta);

    let base_env = setup_env(meta, rootfs, false, None, app_config)?;
    let resolve = make_resolve(rootfs, false);

    let mut children = Vec::new();
    for svc in &meta.services {
        let mut prog = resolve(&svc.cmd[0]);
        if !is_executable_path(&prog) {
            prog = find_in_bin_paths(rootfs, &svc.cmd[0]).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("[xbin] error: service '{}' not found", svc.cmd[0]),
                )
            })?;
        }
        let mut argv: Vec<std::ffi::OsString> = vec![prog.as_os_str().to_os_string()];
        for a in &svc.cmd[1..] {
            argv.push(resolve(a).into_os_string());
        }
        let mut env = base_env.clone();
        for (k, v) in &svc.env {
            env.insert(k.clone(), v.replace("${ROOTFS}", &rootfs.to_string_lossy()));
        }
        let child = crate::win::spawn(&prog, &argv, &env, None, false)?;
        if verbose {
            eprintln!("[xbin] service '{}' started (pid {})", svc.name, child.pid);
        }
        children.push((svc.name.clone(), child));
    }

    wait_for_health(meta, verbose)?;

    let mut exit_code = 0i32;
    for (name, child) in &children {
        let code = crate::win::wait(child)?;
        if verbose {
            eprintln!("[xbin] service '{}' exited with code {}", name, code);
        }
        if code != 0 && exit_code == 0 {
            exit_code = code;
        }
    }
    if exit_code != 0 {
        exit(exit_code);
    }
    Ok(())
}

/// Fork+exec each service, returning (name, pid) pairs.
#[cfg(unix)]
pub fn fork_services(
    meta: &Metadata,
    base_env: &BTreeMap<String, String>,
    resolve: &dyn Fn(&str) -> PathBuf,
    rootfs: &Path,
    verbose: bool,
) -> io::Result<Vec<(String, i32)>> {
    let mut children = Vec::new();
    for svc in &meta.services {
        let prog = resolve(&svc.cmd[0]);
        let prog_c = cstr(prog.as_os_str().as_bytes())?;

        let mut argv: Vec<CString> = Vec::new();
        argv.push(prog_c.clone());
        for a in &svc.cmd[1..] {
            argv.push(cstr(resolve(a).as_os_str().as_bytes())?);
        }

        let mut env = base_env.clone();
        for (k, v) in &svc.env {
            env.insert(k.clone(), v.replace("${ROOTFS}", &rootfs.to_string_lossy()));
        }
        let env_c = env_to_cstrings(&env)?;

        // SAFETY: fork(2) creates a copy of the calling process. The child
        // calls execve (which never returns on success) or exit(127).
        // The parent records the pid for waitpid tracking.
        unsafe {
            let pid = libc::fork();
            if pid < 0 {
                return Err(io::Error::last_os_error());
            }
            if pid == 0 {
                let argv_ptrs = to_ptr_vec(&argv);
                let env_ptrs = to_ptr_vec(&env_c);
                // SAFETY: execve(2) replaces the child process. All pointers
                // are valid CStrings, envp is null-terminated.
                crate::libc_execve(prog_c.as_ptr(), argv_ptrs.as_ptr(), env_ptrs.as_ptr());
                eprintln!(
                    "[xbin] failed to exec {}: {}",
                    svc.cmd[0],
                    io::Error::last_os_error()
                );
                std::process::exit(127);
            }
            if verbose {
                eprintln!("[xbin] service '{}' started (pid {})", svc.name, pid);
            }
            children.push((svc.name.clone(), pid));
        }
    }
    Ok(children)
}

/// Block until all services with `ready_port` are accepting connections.
pub fn wait_for_health(meta: &Metadata, verbose: bool) -> io::Result<()> {
    for svc in &meta.services {
        if svc.ready_port == 0 {
            continue;
        }
        let timeout = if svc.ready_timeout > 0 {
            svc.ready_timeout
        } else {
            30
        };
        if verbose {
            eprintln!(
                "[xbin] waiting for {}:{} (timeout {}s)",
                svc.name, svc.ready_port, timeout
            );
        }
        wait_for_port(svc.ready_port, timeout)?;
        if verbose {
            eprintln!("[xbin] {}:{} is ready", svc.name, svc.ready_port);
        }
    }
    Ok(())
}

/// Wait for all children to exit. Forward SIGTERM/SIGINT to children.
/// Returns the exit code of the first failed service, or 0 if all succeeded.
#[cfg(unix)]
pub fn wait_for_children(children: &[(String, i32)], verbose: bool) -> io::Result<()> {
    let mut exit_code = 0i32;
    let mut remaining = children.len();
    while remaining > 0 {
        let mut status: i32 = 0;
        // SAFETY: waitpid(2) with pid=-1 waits for any child. status is
        // filled by the kernel. We only read it after a successful return.
        let pid = unsafe { libc::waitpid(-1, &mut status, 0) };
        if pid < 0 {
            break;
        }
        remaining -= 1;

        if let Some((name, _)) = children.iter().find(|(_, p)| *p == pid) {
            if libc::WIFEXITED(status) {
                let code = libc::WEXITSTATUS(status);
                if verbose {
                    eprintln!("[xbin] service '{}' exited with code {}", name, code);
                }
                if code != 0 && exit_code == 0 {
                    exit_code = code;
                }
            } else if libc::WIFSIGNALED(status) {
                let sig = libc::WTERMSIG(status);
                eprintln!("[xbin] service '{}' killed by signal {}", name, sig);
                if exit_code == 0 {
                    exit_code = 128 + sig;
                }
                // One service died: kill the rest.
                for (_, cp) in children {
                    if *cp != pid {
                        // SAFETY: kill(2) sends a signal to a process we own
                        // (forked from us). SIGTERM is a graceful shutdown.
                        unsafe {
                            libc::kill(*cp, libc::SIGTERM);
                        }
                    }
                }
            }
        }
    }
    if exit_code != 0 {
        exit(exit_code);
    }
    Ok(())
}

pub fn wait_for_port(port: u16, timeout_secs: u64) -> io::Result<()> {
    use std::net::TcpStream;
    use std::time::{Duration, Instant};
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(_) => return Ok(()),
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("service port {port} not ready within {timeout_secs}s: {e}"),
                ))
            }
        }
    }
}

#[cfg(unix)]
use std::sync::{Mutex, OnceLock};

#[cfg(unix)]
static CHILD_PIDS: OnceLock<Mutex<Vec<i32>>> = OnceLock::new();

#[cfg(unix)]
pub fn install_signal_handler(children: &[(String, i32)]) {
    let pids: Vec<i32> = children.iter().map(|(_, p)| *p).collect();
    CHILD_PIDS.set(Mutex::new(pids)).ok();

    // SAFETY: signal(2) registers a C function pointer as a signal handler.
    // signal_forward only calls kill(2) (async-signal-safe) and reads CHILD_PIDS
    // via OnceLock (initialized before signal handler registration).
    unsafe {
        libc::signal(libc::SIGTERM, signal_forward as *const () as usize);
        libc::signal(libc::SIGINT, signal_forward as *const () as usize);
    }
}

#[cfg(unix)]
extern "C" fn signal_forward(sig: i32) {
    // SAFETY: Called from a signal handler context. Only uses kill(2)
    // (async-signal-safe) and reads CHILD_PIDS via OnceLock.
    // OnceLock::get() is safe to call from signal handler after initialization.
    // Mutex::lock() is async-signal-unsafe in general, but we only call it
    // after the process has been single-threaded (post-fork) or during shutdown
    // when no other threads exist. This is a best-effort cleanup.
    if let Some(mutex) = CHILD_PIDS.get() {
        if let Ok(pids) = mutex.lock() {
            for &pid in pids.iter() {
                // SAFETY: kill(2) with valid PID and signal is async-signal-safe.
                // PID comes from our forked children, signal is from handler argument.
                unsafe {
                    libc::kill(pid, sig);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_resolve_returns_absolute_when_pivot() {
        let rootfs = PathBuf::from("/app");
        let resolve = make_resolve(&rootfs, true);
        assert_eq!(
            resolve("/usr/bin/python3"),
            PathBuf::from("/usr/bin/python3")
        );
    }

    #[test]
    fn make_resolve_strips_leading_slash_when_no_pivot() {
        let rootfs = PathBuf::from("/tmp/xbin-cache/abc");
        let resolve = make_resolve(&rootfs, false);
        assert_eq!(resolve("/usr/bin/python3"), rootfs.join("usr/bin/python3"));
    }

    #[test]
    fn check_executable_returns_false_for_missing_path() {
        assert!(!check_executable("/nonexistent/binary"));
    }

    #[test]
    fn detect_web_port_returns_8000_for_django() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("manage.py"), "").unwrap();
        assert_eq!(detect_web_port(tmp.path(), "python"), Some(8000));
    }

    #[test]
    fn detect_web_port_returns_3000_for_rails() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("bin")).unwrap();
        std::fs::write(tmp.path().join("bin").join("rails"), "").unwrap();
        assert_eq!(detect_web_port(tmp.path(), "ruby"), Some(3000));
    }

    #[test]
    fn detect_django_settings_handles_commas_and_parens() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("manage.py"),
            "os.environ.setdefault(\"DJANGO_SETTINGS_MODULE\", \"myproject.settings\")\n",
        )
        .unwrap();
        assert_eq!(
            detect_django_settings(tmp.path()),
            Some("myproject.settings".to_string())
        );
    }

    #[test]
    fn detect_web_port_handles_port_equals_syntax() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("package.json"),
            r#"{"scripts":{"start":"next start --port=3000"}}"#,
        )
        .unwrap();
        assert_eq!(detect_web_port(tmp.path(), "node"), Some(3000));
    }
}
