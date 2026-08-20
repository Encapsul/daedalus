//! Process execution for the erebus launcher stub.
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
use erebus_core::detect;

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
/// When `orig_cwd` is Some, inserts `ERE_ORIG_CWD` (used by single-service exec).
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
        env.insert("ERE_ORIG_CWD".into(), cwd.to_string_lossy().into_owned());
    }

    // App-bundled `.env` is the LOWEST-priority source: every explicit
    // override below (meta.env, config secrets, DATABASE_URL) wins on
    // collision, so a packaged `.env` can't silently shadow operator config.
    for (k, v) in erebus_core::dotenv::load_dotenv(rootfs, None, false) {
        env.entry(k).or_insert(v);
    }

    let rootfs_str = rootfs.to_string_lossy();
    for (k, v) in &meta.env {
        env.insert(k.clone(), v.replace("${ROOTFS}", &rootfs_str));
    }

    // Merge secrets from config file
    if let Some(secrets) = &app_config.secrets {
        for (k, v) in secrets {
            env.insert(format!("ERE_SECRET_{}", k.to_uppercase()), v.clone());
        }
    }

    // Merge database URL from config
    if let Some(url) = app_config.get_database_url() {
        env.insert("DATABASE_URL".into(), url);
    }

    // Framework-specific defaults (fill gaps only — .env and operator config win)
    match meta.runtime.as_str() {
        "python" => {
            if !env.contains_key("PYTHONUNBUFFERED") {
                env.insert("PYTHONUNBUFFERED".into(), "1".into());
            }
            if !env.contains_key("DJANGO_SETTINGS_MODULE") {
                if let Some(settings) = detect_django_settings(rootfs) {
                    env.insert("DJANGO_SETTINGS_MODULE".into(), settings);
                }
            }
        }
        "node" => {
            if !env.contains_key("NODE_ENV") {
                env.insert("NODE_ENV".into(), "production".into());
            }
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
            if !env.contains_key("RAILS_ENV") {
                env.insert("RAILS_ENV".into(), "production".into());
            }
            if !env.contains_key("RACK_ENV") {
                env.insert("RACK_ENV".into(), "production".into());
            }
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
        if let Err(e) = std::fs::create_dir_all(&path) {
            eprintln!("[erebus] warning: could not create {}: {e}", path.display());
        }
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

/// Expand `$VAR` / `${VAR}` placeholders in an argv argument from the child env.
///
/// Returns `None` when a referenced variable is unset so the caller can drop
/// the argument (e.g. `-Dserver.port=$PORT` for a Java app with no web port).
pub fn expand_env_arg(arg: &str, env: &BTreeMap<String, String>) -> Option<String> {
    if !arg.contains('$') {
        return Some(arg.to_string());
    }
    let mut out = String::with_capacity(arg.len());
    let mut rest = arg;
    while let Some(idx) = rest.find('$') {
        out.push_str(&rest[..idx]);
        rest = &rest[idx + 1..];
        if rest.starts_with('{') {
            let Some(end) = rest.find('}') else {
                // Unbalanced `${` — keep the literal as-is.
                out.push_str(rest);
                rest = "";
                break;
            };
            out.push_str(env.get(&rest[1..end])?);
            rest = &rest[end + 1..];
        } else {
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            let name = &rest[..end];
            if name.is_empty() {
                // Trailing `$` — keep it literal.
                out.push('$');
            } else {
                out.push_str(env.get(name)?);
            }
            rest = &rest[end..];
        }
    }
    out.push_str(rest);
    Some(out)
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
    let path = match std::str::from_utf8(prog) {
        Ok(s) => s,
        Err(_) => return false,
    };
    // If it's an absolute or relative path with a directory component, check it directly.
    if path.contains('/') || path.contains('\\') {
        return check_executable(path);
    }
    // Otherwise search PATH directories (mirrors execvp behavior).
    let paths = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(std::path::Path::new(path));
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

/// Whether `prog` is runnable: on the host PATH or, for a bare interpreter
/// name, inside the rootfs bin dirs (embedded-interpreter case). With
/// `pivot_root` the rootfs PATH takes over before `execvp`, so a rootfs-only
/// interpreter must pass the pre-flight even when the host lacks the runtime.
#[cfg(unix)]
fn entrypoint_is_executable(prog: &[u8], interpreter_name: &str, rootfs: &Path) -> bool {
    if is_executable(prog) {
        return true;
    }
    // Absolute/relative paths can't fall back to the rootfs search.
    if prog.contains(&b'/') {
        return false;
    }
    crate::BIN_PATHS.iter().any(|dir| {
        let candidate = rootfs.join(dir).join(interpreter_name);
        check_executable(&candidate.to_string_lossy())
    })
}

// ---------------------------------------------------------------------------
// Single-service exec
// ---------------------------------------------------------------------------

/// Bind-mount X11, Wayland, and GPU devices into the rootfs before
/// `pivot_root`. Each path is created with `mkdir -p` then `mount MS_BIND`.
///
/// Source paths are best-effort: non-existent sources (e.g. no Wayland
/// socket) are silently skipped. This lets the same binary run on X11-only,
/// Wayland-only, or mixed systems.
#[cfg(target_os = "linux")]
fn bind_mount_gui_devices(rootfs: &Path) -> io::Result<()> {
    // SAFETY: getuid(2) returns the real UID of the calling process. It
    // cannot fail and has no invalid inputs.
    let uid = unsafe { libc::getuid() };
    let run_user = format!("/run/user/{uid}");

    // (host_path, rootfs_relative_dest) pairs to bind-mount.
    let mounts: &[(&str, &str)] = &[("/tmp/.X11-unix", ".X11-unix"), ("/dev/dri", "dev/dri")];

    for (src, dest_rel) in mounts {
        let dest = rootfs.join(dest_rel);
        if !Path::new(src).exists() {
            continue;
        }
        std::fs::create_dir_all(&dest)?;
        let src_c = crate::cstr(src.as_bytes())?;
        let dest_c = crate::cstr(dest.as_os_str().as_bytes())?;
        // SAFETY: mount(2) with MS_BIND makes a 1:1 bind mount of the host
        // GUI socket/GPU device into the rootfs. Source and destination are
        // valid paths; no data pointer needed for bind mounts.
        let rc = unsafe {
            libc::mount(
                src_c.as_ptr(),
                dest_c.as_ptr(),
                std::ptr::null(),
                libc::MS_BIND,
                std::ptr::null(),
            )
        };
        if rc != 0 {
            eprintln!(
                "[erebus] warning: failed to bind-mount {} → {}: {}",
                src,
                dest.display(),
                io::Error::last_os_error()
            );
        }
    }

    // Wayland sockets glob /run/user/$UID/.wayland-*
    if Path::new(&run_user).is_dir() {
        let dest_dir = rootfs.join(&run_user);
        if let Ok(entries) = std::fs::read_dir(&run_user) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if !name.starts_with("wayland-") && !name.starts_with(".wayland-") {
                    continue;
                }
                let src = entry.path();
                let dest = dest_dir.join(std::ffi::OsStr::new(&*name));
                std::fs::create_dir_all(&dest)?;
                let src_c = crate::cstr(src.to_string_lossy().as_bytes())?;
                let dest_c = crate::cstr(dest.as_os_str().as_bytes())?;
                // SAFETY: bind-mount a single Wayland socket into rootfs.
                let rc = unsafe {
                    libc::mount(
                        src_c.as_ptr(),
                        dest_c.as_ptr(),
                        std::ptr::null(),
                        libc::MS_BIND,
                        std::ptr::null(),
                    )
                };
                if rc != 0 {
                    eprintln!(
                        "[erebus] warning: failed to bind-mount {}: {}",
                        src.display(),
                        io::Error::last_os_error()
                    );
                }
            }
        }
    }

    // GPU render nodes (dri/card*)
    if Path::new("/dev/dri").is_dir() {
        let src_dir = "/dev/dri";
        let dest_dir = rootfs.join("dev/dri");
        std::fs::create_dir_all(&dest_dir)?;
        if let Ok(entries) = std::fs::read_dir(src_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                // Only bind-mount render nodes and control nodes
                if !name.starts_with("renderD") && !name.starts_with("card") {
                    continue;
                }
                let src = format!("{src_dir}/{name}");
                let dest = dest_dir.join(std::ffi::OsStr::new(&*name));
                let src_c = crate::cstr(src.as_bytes())?;
                let dest_c = crate::cstr(dest.as_os_str().as_bytes())?;
                // SAFETY: bind-mount GPU device node into rootfs.
                let rc = unsafe {
                    libc::mount(
                        src_c.as_ptr(),
                        dest_c.as_ptr(),
                        std::ptr::null(),
                        libc::MS_BIND,
                        std::ptr::null(),
                    )
                };
                if rc != 0 {
                    eprintln!(
                        "[erebus] warning: failed to bind-mount GPU device {}: {}",
                        src,
                        io::Error::last_os_error()
                    );
                }
            }
        }
    }

    // Ensure /dev/shm is available for X11 shared memory
    if !rootfs.join("dev/shm").is_dir() {
        std::fs::create_dir_all(rootfs.join("dev/shm"))?;
    }

    Ok(())
}

/// Enters the `pivot_root` isolation and installs the requested sandboxes.
///
/// Landlock rules anchor to the rootfs inode, so the `O_PATH` fd is opened
/// BEFORE `pivot_root` replaces the mount tree — afterwards the original
/// rootfs path would no longer resolve and the rule could not be added.
/// Landlock failures are fatal (fail-closed): the filesystem sandbox is the
/// last line of defense of the isolation level, and running the app without
/// it would silently defeat the requested `--landlock` guarantee.
///
/// When `meta.gui` is set, X11/Wayland/GPU devices are bind-mounted into
/// the rootfs **before** `pivot_root` so they remain accessible after the
/// mount tree swap. Bind-mounting must happen before `pivot_root` because the
/// host filesystem is unreachable afterwards.
#[cfg(target_os = "linux")]
fn enter_pivot_sandbox(rootfs: &Path, meta: &Metadata) -> io::Result<()> {
    let root_guard = if meta.landlock {
        Some(crate::landlock::RootfsGuard::open(rootfs)?)
    } else {
        None
    };

    if meta.gui {
        bind_mount_gui_devices(rootfs)?;
    }

    crate::pivot_root_into(rootfs)?;
    if meta.seccomp {
        crate::seccomp::install_seccomp_denylist().map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("seccomp unavailable, refusing to run without syscall filter: {e}"),
            )
        })?;
    }
    if let Some(root) = root_guard {
        crate::landlock::sandbox(&root).map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("landlock unavailable, refusing to run without filesystem sandbox: {e}"),
            )
        })?;
    }
    Ok(())
}

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

    // Sandbox flags only take effect on the pivot_root path (isolation >= 2).
    // Warn on the non-pivot path instead of silently ignoring the request —
    // metadata can be authored by any builder, not just the CLI. (macOS
    // applies its own sandbox profile for `landlock`, so this is Linux-only.)
    #[cfg(target_os = "linux")]
    if !use_pivot && (meta.seccomp || meta.landlock) {
        let flags = ["seccomp", "landlock"]
            .into_iter()
            .filter(|name| {
                (*name == "seccomp" && meta.seccomp) || (*name == "landlock" && meta.landlock)
            })
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!("[erebus] warning: {flags} requested but isolation < 2 — sandbox not applied");
    }

    #[cfg(target_os = "linux")]
    if use_pivot && crate::namespace::running_in_container() {
        eprintln!("[erebus] warning: running inside a container — namespace isolation may be restricted by the host");
    }

    enter_namespace_if_needed(meta.isolation)?;
    #[cfg(target_os = "linux")]
    if use_pivot {
        enter_pivot_sandbox(rootfs, meta)?;
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
        let (prog, direct_exec, interpreter_name) = resolve_entrypoint(meta, rootfs, use_pivot)?;
        let resolve = make_resolve(rootfs, use_pivot);
        let prog_c = cstr(prog.as_os_str().as_bytes())?;
        let prog_path_bytes = prog.as_os_str().as_bytes();

        if !entrypoint_is_executable(prog_path_bytes, &interpreter_name, rootfs) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "[erebus] error: interpreter '{}' not found (tried: {})",
                    interpreter_name,
                    prog.display()
                ),
            ));
        }

        // Compute the child environment before building argv: entrypoint args
        // may carry `$PORT`/`${VAR}` placeholders that must expand to run-time
        // values (e.g. Java's `-Dserver.port=$PORT`).
        let env = setup_env(meta, rootfs, use_pivot, orig_cwd.as_deref(), app_config)?;

        let mut argv: Vec<CString> = Vec::new();
        if direct_exec {
            argv.push(prog_c.clone());
            for a in &meta.entrypoint[1..] {
                if let Some(expanded) = expand_env_arg(a, &env) {
                    argv.push(cstr(resolve(&expanded).as_os_str().as_bytes())?);
                }
            }
        } else {
            if !entrypoint_is_executable(interpreter_name.as_bytes(), &interpreter_name, rootfs) {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!(
                        "[erebus] error: interpreter '{}' not found",
                        interpreter_name
                    ),
                ));
            }
            argv.push(cstr(interpreter_name.as_bytes())?);
            for a in &meta.entrypoint[1..] {
                if let Some(expanded) = expand_env_arg(a, &env) {
                    argv.push(cstr(resolve(&expanded).as_os_str().as_bytes())?);
                }
            }
        }
        for a in std::env::args_os().skip(1) {
            argv.push(cstr(a.as_bytes())?);
        }

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
/// Canonical interpreter binary name for each interpreted runtime. Compiled
/// runtimes (`go`, `binary`) return `None` — their entrypoint is the program.
fn runtime_interpreter(runtime: &str) -> Option<&'static str> {
    match runtime {
        "python" => Some("python3"),
        "node" => Some("node"),
        "electron" => Some("electron"),
        "deno" => Some("deno"),
        "java" => Some("java"),
        "ruby" => Some("ruby"),
        "dotnet" => Some("dotnet"),
        "php" => Some("php"),
        "perl" => Some("perl"),
        "hugo" => Some("hugo"),
        "wasm" => Some("wasmtime"),
        _ => None,
    }
}

/// Resolve the entrypoint program and interpreter for `meta`, validated against
/// the supported runtime list. `None` from `Runtime::from_name` means the
/// metadata carries a runtime the stub never heard of — fail loudly instead of
/// silently launching under bash (upstream fix for roadmap #40).
pub fn resolve_entrypoint(
    meta: &Metadata,
    rootfs: &Path,
    use_pivot: bool,
) -> io::Result<(PathBuf, bool, String)> {
    if detect::Runtime::from_name(&meta.runtime).is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported runtime '{}' in metadata — supported: python, deno, node, electron, java, ruby, dotnet, go, php, perl, hugo, wasm, binary",
                meta.runtime
            ),
        ));
    }
    let resolve = make_resolve(rootfs, use_pivot);
    let mut prog = resolve(&meta.entrypoint[0]);

    // Compiled binaries (go/binary) exec `entrypoint[0]` directly; interpreted
    // runtimes get their interpreter prepended to argv.
    let direct_exec = matches!(meta.runtime.as_str(), "go" | "binary");
    let interpreter_name = runtime_interpreter(&meta.runtime)
        .map(str::to_string)
        .unwrap_or_else(|| {
            meta.entrypoint[0]
                .rsplit('/')
                .next()
                .unwrap_or(&meta.entrypoint[0])
                .to_string()
        });

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

    Ok((prog, direct_exec, interpreter_name))
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
    let (prog, direct_exec, interpreter_name) = resolve_entrypoint(meta, rootfs, false)?;
    let env = setup_env(meta, rootfs, false, None, app_config)?;
    let cwd = meta.cwd.as_ref().map(|c| make_resolve(rootfs, false)(c));
    let resolve = make_resolve(rootfs, false);

    let mut cmd = prog;
    if !is_executable_path(&cmd) {
        cmd = find_in_bin_paths(rootfs, &meta.entrypoint[0]).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "[erebus] error: interpreter '{}' not found (tried: {})",
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
        if let Some(expanded) = expand_env_arg(a, &env) {
            argv.push(resolve(&expanded).into_os_string());
        }
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
    let verbose = std::env::var_os("ERE_VERBOSE").is_some();
    #[cfg(target_os = "linux")]
    let use_pivot = meta.isolation >= 2;
    #[cfg(not(target_os = "linux"))]
    let use_pivot = false;

    enter_namespace_if_needed(meta.isolation)?;
    #[cfg(target_os = "linux")]
    if use_pivot {
        enter_pivot_sandbox(rootfs, meta)?;
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
    // Start health gate AFTER fork to avoid fork() while thread is running
    // (fork in multithreaded process can deadlock child via held libc locks).
    crate::health_gate::maybe_start_health(meta);
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
    let verbose = std::env::var_os("ERE_VERBOSE").is_some();

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
                    format!("[erebus] error: service '{}' not found", svc.cmd[0]),
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
            eprintln!(
                "[erebus] service '{}' started (pid {})",
                svc.name, child.pid
            );
        }
        children.push((svc.name.clone(), child));
    }

    wait_for_health(meta, verbose)?;

    let mut exit_code = 0i32;
    for (name, child) in &children {
        let code = crate::win::wait(child)?;
        if verbose {
            eprintln!("[erebus] service '{}' exited with code {}", name, code);
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
                    "[erebus] failed to exec {}: {}",
                    svc.cmd[0],
                    io::Error::last_os_error()
                );
                std::process::exit(127);
            }
            if verbose {
                eprintln!("[erebus] service '{}' started (pid {})", svc.name, pid);
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
                "[erebus] waiting for {}:{} (timeout {}s)",
                svc.name, svc.ready_port, timeout
            );
        }
        wait_for_port(svc.ready_port, timeout)?;
        if verbose {
            eprintln!("[erebus] {}:{} is ready", svc.name, svc.ready_port);
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
            // ECHILD with services still tracked means the children were
            // reaped elsewhere (or never spawned) — report it instead of
            // silently "succeeding" while services are gone. Any other
            // error just stops the wait loop.
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::ECHILD) && remaining > 0 {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("all children gone but {remaining} tracked service(s) not reaped"),
                ));
            }
            break;
        }

        // waitpid(-1, ...) also reaps spurious children (grandchildren,
        // helpers). Only a pid we actually supervise may decrement the
        // tracked count, or the loop would exit early with services alive.
        let Some((name, _)) = children.iter().find(|(_, p)| *p == pid) else {
            continue;
        };
        remaining -= 1;

        if libc::WIFEXITED(status) {
            let code = libc::WEXITSTATUS(status);
            if verbose {
                eprintln!("[erebus] service '{}' exited with code {}", name, code);
            }
            if code != 0 && exit_code == 0 {
                exit_code = code;
            }
        } else if libc::WIFSIGNALED(status) {
            let sig = libc::WTERMSIG(status);
            eprintln!("[erebus] service '{}' killed by signal {}", name, sig);
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
use std::sync::atomic::{AtomicUsize, Ordering};

/// Maximum number of child PIDs the signal handler can forward to.
/// Fixed-size array avoids heap allocation and Mutex in signal context.
#[cfg(unix)]
const MAX_CHILDREN: usize = 64;

#[cfg(unix)]
static CHILD_PID_COUNT: AtomicUsize = AtomicUsize::new(0);

#[cfg(unix)]
static mut CHILD_PIDS: [i32; MAX_CHILDREN] = [0; MAX_CHILDREN];

/// Install SIGTERM/SIGINT forwarding to child processes.
///
/// # Safety precondition
///
/// This function registers a signal handler that reads `CHILD_PIDS` via
/// `static mut` (safe only under the single-threaded invariant below).
/// Callers MUST ensure the process is single-threaded at this point —
/// no spawned threads, no concurrent mutable access to `CHILD_PIDS`.
#[cfg(unix)]
pub fn install_signal_handler(children: &[(String, i32)]) {
    let count = children.len().min(MAX_CHILDREN);
    // SAFETY: install_signal_handler is called from a single-threaded context
    // (post-fork, no spawned threads). The loop writes [0..count] and the
    // count is bounded by MAX_CHILDREN. No concurrent access exists.
    unsafe {
        for (i, (_, pid)) in children.iter().take(count).enumerate() {
            CHILD_PIDS[i] = *pid;
        }
        CHILD_PID_COUNT.store(count, Ordering::Relaxed);
    }

    // SAFETY: signal(2) registers a C function pointer as a signal handler.
    // signal_forward only calls kill(2) (async-signal-safe) and reads
    // CHILD_PIDS/CHILD_PID_COUNT which are initialized above.
    unsafe {
        libc::signal(libc::SIGTERM, signal_forward as *const () as usize);
        libc::signal(libc::SIGINT, signal_forward as *const () as usize);
    }
}

#[cfg(unix)]
extern "C" fn signal_forward(sig: i32) {
    // SAFETY: signal_forward runs in signal handler context. All operations
    // are async-signal-safe: Relaxed atomic load, libc::kill(2), and reading
    // CHILD_PIDS/CHILD_PID_COUNT which are initialized before handler
    // registration. Single-threaded invariant enforced by install_signal_handler.
    let count = CHILD_PID_COUNT.load(Ordering::Relaxed);
    unsafe {
        let ptr = core::ptr::addr_of!(CHILD_PIDS);
        for i in 0..count {
            libc::kill((*ptr)[i], sig);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DatabaseConfig;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    use std::sync::Mutex;

    /// Serializes fork-based tests (see `wait_for_children` docs).
    static FORK_TEST_LOCK: Mutex<()> = Mutex::new(());

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
        let rootfs = PathBuf::from("/tmp/erebus-cache/abc");
        let resolve = make_resolve(&rootfs, false);
        assert_eq!(resolve("/usr/bin/python3"), rootfs.join("usr/bin/python3"));
    }

    #[test]
    fn check_executable_returns_false_for_missing_path() {
        assert!(!check_executable("/nonexistent/binary"));
    }

    #[test]
    fn entrypoint_executable_falls_back_to_rootfs_interpreter() {
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = tmp.path();
        std::fs::create_dir_all(rootfs.join("usr/bin")).unwrap();
        let interp = rootfs.join("usr/bin/python3");
        std::fs::write(&interp, b"#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&interp, std::fs::Permissions::from_mode(0o755)).unwrap();

        // Embedded interpreter exists only in the rootfs: the pre-flight must
        // still pass when the host PATH has no `python3`.
        assert!(entrypoint_is_executable(b"python3", "python3", rootfs));
    }

    #[test]
    fn entrypoint_executable_rejects_absolute_missing_path() {
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = tmp.path();
        std::fs::create_dir_all(rootfs.join("usr/bin")).unwrap();
        std::fs::write(rootfs.join("usr/bin/python3"), b"x").unwrap();
        // An absolute prog path cannot fall back to the rootfs search.
        assert!(!entrypoint_is_executable(
            b"/app/python3",
            "python3",
            rootfs
        ));
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
    fn setup_env_dotenv_loses_to_explicit_config() {
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = tmp.path();
        std::fs::write(
            rootfs.join(".env"),
            "DATABASE_URL=postgres://dotenv/db\nMY_KEY=dotenv\nNODE_ENV=staging",
        )
        .unwrap();

        let meta = Metadata {
            name: "test".into(),
            version: None,
            runtime: "node".into(),
            entrypoint: Vec::new(),
            env: {
                let mut m = BTreeMap::new();
                m.insert("MY_KEY".to_string(), "explicit".to_string());
                m
            },
            cwd: None,
            isolation: 0,
            seccomp: false,
            gui: false,
            landlock: false,
            services: Vec::new(),
            crypto: None,
            payload_format: String::new(),
            health_check: None,
            update_url: None,
        };

        let app_config = AppConfig {
            database: Some(DatabaseConfig {
                url: Some("postgres://config/db".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let env = setup_env(&meta, rootfs, false, None, &app_config).unwrap();

        // Explicit --env wins over the app's .env.
        assert_eq!(env.get("MY_KEY").map(String::as_str), Some("explicit"));
        // Config DATABASE_URL wins over the app's .env.
        assert_eq!(
            env.get("DATABASE_URL").map(String::as_str),
            Some("postgres://config/db")
        );
        // .env still fills gaps; the node production default loses to it.
        assert_eq!(env.get("NODE_ENV").map(String::as_str), Some("staging"));
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

    #[test]
    fn wait_for_children_ignores_spurious_and_waits_for_tracked() {
        // Serialized: waitpid(-1, ...) reaps ANY child of the process, so
        // fork-based tests must not run concurrently or they steal each
        // other's children.
        let _guard = FORK_TEST_LOCK.lock().unwrap();
        // SAFETY: fork(2) duplicates the test process; the children only call
        // _exit(0), which is async-signal-safe and safe after forking a
        // multithreaded process.
        let spurious = unsafe { libc::fork() };
        assert!(spurious >= 0, "fork failed");
        if spurious == 0 {
            unsafe { libc::_exit(0) };
        }

        let tracked = unsafe { libc::fork() };
        assert!(tracked >= 0, "fork failed");
        if tracked == 0 {
            std::thread::sleep(std::time::Duration::from_millis(300));
            unsafe { libc::_exit(0) };
        }

        let start = std::time::Instant::now();
        wait_for_children(&[("tracked".to_string(), tracked)], false).unwrap();

        // Reaping the spurious child must not advance the tracked count: the
        // call only returns once the tracked service has actually exited.
        assert!(
            start.elapsed() >= std::time::Duration::from_millis(250),
            "returned before the tracked service exited"
        );
    }

    #[test]
    fn wait_for_children_ok_when_tracked_exits_zero() {
        // Serialized: see wait_for_children_ignores_spurious_and_waits_for_tracked.
        let _guard = FORK_TEST_LOCK.lock().unwrap();
        // SAFETY: fork(2) duplicates the test process; the child only calls
        // _exit(0), which is async-signal-safe and safe after forking a
        // multithreaded process.
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork failed");
        if pid == 0 {
            unsafe { libc::_exit(0) };
        }

        wait_for_children(&[("ok".to_string(), pid)], false).unwrap();
    }

    fn env_map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn expand_env_arg_expands_port() {
        let env = env_map(&[("PORT", "8080")]);
        assert_eq!(
            expand_env_arg("-Dserver.port=$PORT", &env),
            Some("-Dserver.port=8080".to_string())
        );
    }

    #[test]
    fn expand_env_arg_supports_braces() {
        let env = env_map(&[("ERE_PORT", "9000")]);
        assert_eq!(
            expand_env_arg("-Dserver.port=${ERE_PORT}", &env),
            Some("-Dserver.port=9000".to_string())
        );
    }

    #[test]
    fn expand_env_arg_drops_arg_when_var_unset() {
        assert_eq!(expand_env_arg("-Dserver.port=$PORT", &env_map(&[])), None);
    }

    #[test]
    fn expand_env_arg_passes_through_without_dollar() {
        let env = env_map(&[("PORT", "8080")]);
        assert_eq!(
            expand_env_arg("/app/app.jar", &env),
            Some("/app/app.jar".to_string())
        );
    }

    #[test]
    fn expand_env_arg_handles_trailing_dollar() {
        let env = env_map(&[("PORT", "8080")]);
        assert_eq!(expand_env_arg("cost$", &env), Some("cost$".to_string()));
    }

    #[test]
    fn resolve_entrypoint_rejects_unknown_runtime() {
        let tmp = tempfile::tempdir().unwrap();
        let meta = Metadata {
            name: "crafted".into(),
            version: None,
            runtime: "cobol".into(),
            entrypoint: vec!["/app/app".into()],
            env: BTreeMap::new(),
            cwd: None,
            isolation: 0,
            seccomp: false,
            gui: false,
            landlock: false,
            services: Vec::new(),
            crypto: None,
            payload_format: String::new(),
            health_check: None,
            update_url: None,
        };
        let err = resolve_entrypoint(&meta, tmp.path(), false).unwrap_err();
        assert!(err.to_string().contains("unsupported runtime 'cobol'"));
    }

    #[test]
    fn runtime_interpreter_covers_all_interpreted_runtimes() {
        for name in [
            "python", "node", "electron", "deno", "java", "ruby", "dotnet", "php", "perl", "hugo",
            "wasm",
        ] {
            assert!(runtime_interpreter(name).is_some(), "runtime: {name}");
        }
        for name in ["go", "binary"] {
            assert!(runtime_interpreter(name).is_none(), "runtime: {name}");
        }
        assert!(runtime_interpreter("cobol").is_none());
    }
}
