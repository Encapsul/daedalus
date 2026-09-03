//! Runtime embedder: locates and bundles interpreters, package managers,
//! native libraries, and framework-specific files into the payload.
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// PHP extensions that are built into PHP and don't need external .so files.
const PHP_BUILTIN_EXTENSIONS: &[&str] = &[
    "Core",
    "ctype",
    "date",
    "dom",
    "exif",
    "FFI",
    "fileinfo",
    "filter",
    "ftp",
    "gettext",
    "hash",
    "iconv",
    "json",
    "libxml",
    "mbstring",
    "mysqli",
    "openssl",
    "pcre",
    "PDO",
    "pdo_mysql",
    "pdo_sqlite",
    "phar",
    "posix",
    "readline",
    "session",
    "simplexml",
    "sodium",
    "sqlite3",
    "standard",
    "tokenizer",
    "xml",
    "xmlreader",
    "xmlwriter",
    "zlib",
];

/// Embed an interpreter and its shared library dependencies into a rootfs.
///
/// Returns the number of files copied (interpreter + shared libs + config).
pub fn embed_interpreter(
    interpreter: &str,
    rootfs: &Path,
    app_dir: Option<&Path>,
    verbose: bool,
) -> io::Result<usize> {
    let interp_path = find_interpreter_host(interpreter).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("interpreter '{interpreter}' not found on PATH"),
        )
    })?;
    embed_interpreter_from_path(&interp_path, rootfs, app_dir, verbose)
}

/// Embed an interpreter from an explicit binary path (cross-compile support).
/// `interp_path` points to a target-compatible interpreter (e.g. from a
/// downloaded `python-build-standalone` or Node.js tarball). The binary and
/// its shared-library dependencies are copied into `rootfs/`.
pub fn embed_interpreter_from_path(
    interp_path: &Path,
    rootfs: &Path,
    app_dir: Option<&Path>,
    verbose: bool,
) -> io::Result<usize> {
    if verbose {
        eprintln!("  embed: interpreter found at {}", interp_path.display());
    }

    let mut count = 0;

    // Copy the interpreter binary to rootfs/usr/bin/, keeping the source
    // filename so a Windows `node.exe` lands as `node.exe` (CreateProcessW
    // resolves the bare name via PATH and appends `.exe`).
    let bin_dir = rootfs.join("usr/bin");
    fs::create_dir_all(&bin_dir)?;
    let dest_name = interp_path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("interpreter"));
    let dest_bin = bin_dir.join(dest_name);
    fs::copy(interp_path, &dest_bin)?;
    count += 1;

    // Set executable permission
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dest_bin, fs::Permissions::from_mode(0o755))?;
    }

    if verbose {
        eprintln!(
            "  embed: copied {} to {}",
            interp_path.display(),
            dest_bin.display()
        );
    }

    // Find and copy shared library dependencies via ldd
    let deps = ldd_deps(interp_path)?;
    let mut seen = HashSet::new();
    let lib_dirs = resolve_lib_dirs(rootfs);

    for lib_path in &deps {
        let name = match lib_path.file_name() {
            Some(n) => n,
            None => continue,
        };
        let name_str = name.to_string_lossy();
        if !seen.insert(name_str.to_string()) {
            continue;
        }

        // Determine destination directory based on source path
        let dest_dir = find_lib_dest(lib_path, &lib_dirs);
        fs::create_dir_all(&dest_dir)?;
        let dest_lib = dest_dir.join(name);

        let _ = fs::remove_file(&dest_lib);
        // SAFETY: always copy the dereferenced file content, never preserve
        // symlinks as-is. Host symlinks (e.g. libstdc++.so.6 ->
        // libstdc++.so.6.0.33) would be broken in rootfs because the
        // symlink target is never separately embedded. fs::copy follows
        // symlinks and writes the real file content under the SONAME name,
        // keeping the dynamic linker's NEEDED lookup working.
        fs::copy(lib_path, &dest_lib)?;
        count += 1;

        if verbose {
            eprintln!("  embed: lib {} -> {}", name_str, dest_dir.display());
        }
    }

    // Copy the ELF interpreter (dynamic loader) so the embedded binary can
    // be exec'd under `pivot_root` isolation, where the host /lib64 is gone.
    if let Some(loader) = elf_interpreter_path(interp_path)? {
        let dest = rootfs.join(loader.strip_prefix("/").unwrap_or(&loader));
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&loader, &dest)?;
        count += 1;
        if verbose {
            eprintln!(
                "  embed: ELF interpreter {} -> {}",
                loader.display(),
                dest.display()
            );
        }
    }

    // Copy runtime-specific config (php.ini, extensions, etc.)
    // Infer interpreter name from the binary filename for config matching.
    let interp_name = interp_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");
    count += embed_runtime_config(interp_name, interp_path, rootfs, app_dir, verbose)?;

    Ok(count)
}

/// Find an interpreter binary on the system PATH (host fallback).
pub fn find_interpreter_host(name: &str) -> Option<PathBuf> {
    // Cross-compilation: the target runtime is staged as `<name>.exe` in a
    // tools dir prepended to PATH (e.g. `node.exe` for a win-x64 build), so
    // try the suffixed form first — it wins over any host interpreter and is
    // a no-op miss on POSIX hosts that have no `.exe` binaries on PATH.
    //
    // Iterate `PATH` directly instead of shelling out to `which`, which does
    // not resolve on Windows runners that expose only `python.exe`/`node.exe`.
    let path = std::env::var_os("PATH")?;
    let candidates = [format!("{name}.exe"), name.to_string()];
    for candidate in &candidates {
        for dir in std::env::split_paths(&path) {
            let full = dir.join(candidate);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
}

/// Parse `ldd` output to find shared library paths for a binary or .node file.
pub(crate) fn ldd_deps(interp_path: &Path) -> io::Result<Vec<PathBuf>> {
    // `ldd` is a Linux ELF tool. On Windows the interpreter (e.g. the official
    // node.exe) is a self-contained PE that resolves its DLLs via PATH and the
    // system dirs, so there is nothing to embed here. Return empty instead of
    // aborting the whole embed.
    #[cfg(not(unix))]
    {
        let _ = interp_path;
        Ok(Vec::new())
    }
    #[cfg(unix)]
    {
        let output = Command::new("ldd")
            .arg(interp_path)
            .output()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("ldd failed: {e}")))?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut deps = Vec::new();

        for line in stdout.lines() {
            let line = line.trim();
            // Skip empty lines, vdso, and ld-linux dynamic linker (handled separately)
            if line.is_empty() || line.starts_with("linux-vdso") {
                continue;
            }

            let path_str = if let Some(after) = line.split("=>").nth(1) {
                // glibc format: "libfoo.so.1 => /path/libfoo.so.1 (0x...)"
                after.trim()
            } else if line.starts_with('/') {
                // musl format: "/path/libfoo.so.1" (no => prefix)
                line
            } else {
                continue;
            };

            // Skip the dynamic linker (ld-linux, ld-musl) — it's not a library dep
            let fname = path_str.rsplit('/').next().unwrap_or(path_str);
            if fname.starts_with("ld-linux") || fname.starts_with("ld-musl") {
                continue;
            }

            // Remove the address part: "(0x...)"
            let path_str = if let Some(idx) = path_str.find(" (") {
                &path_str[..idx]
            } else {
                path_str
            };
            let path_str = path_str.trim();
            if !path_str.starts_with('/') || path_str == "not found" {
                continue;
            }
            deps.push(PathBuf::from(path_str));
        }

        Ok(deps)
    }
}

/// Extract the ELF interpreter (dynamic loader) path from a single `ldd`
/// output line. The loader line has no `=>`, e.g.
/// `/lib64/ld-linux-x86-64.so.2 (0x00007f...)`.
#[cfg(unix)]
fn parse_loader_line(line: &str) -> Option<PathBuf> {
    let line = line.trim();
    if !line.starts_with('/') || line.contains("=>") {
        return None;
    }
    let path_str = line.split('(').next()?.trim();
    if path_str.contains("ld-linux") || path_str.contains("ld-musl") {
        Some(PathBuf::from(path_str))
    } else {
        None
    }
}

/// Locate the ELF interpreter required by `interp_path` (e.g. glibc's
/// `/lib64/ld-linux-x86-64.so.2`). Without it in the rootfs, the kernel
/// cannot exec the embedded binary under `pivot_root` isolation.
fn elf_interpreter_path(interp_path: &Path) -> io::Result<Option<PathBuf>> {
    #[cfg(not(unix))]
    {
        let _ = interp_path;
        // No ELF dynamic loader on Windows; the interpreter is a self-contained
        // PE that resolves its DLLs via PATH/system dirs.
        Ok(None)
    }
    #[cfg(unix)]
    {
        let output = match Command::new("ldd").arg(interp_path).output() {
            Ok(o) => o,
            // ldd unavailable (or spawn failed); leave the loader un-embedded
            // rather than aborting the whole embed.
            Err(_) => return Ok(None),
        };
        if !output.status.success() {
            return Ok(None);
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(parse_loader_line))
    }
}
/// `resolve_lib_dirs` - resolve lib dirs.
/// `@rootfs`: rootfs
///
/// Description:
///
/// Return: vector of Vec<PathBuf>
fn resolve_lib_dirs(rootfs: &Path) -> Vec<PathBuf> {
    let arch = std::env::consts::ARCH;
    let arch_dir = match arch {
        "x86_64" => "x86_64-linux-gnu",
        "aarch64" => "aarch64-linux-gnu",
        _ => "",
    };

    let mut dirs = vec![
        rootfs.join("lib"),
        rootfs.join("lib64"),
        rootfs.join("usr/lib"),
        rootfs.join("usr/lib64"),
    ];

    if !arch_dir.is_empty() {
        dirs.push(rootfs.join("usr/lib").join(arch_dir));
        dirs.push(rootfs.join("lib").join(arch_dir));
    }

    dirs
}

/// Find the best destination directory for a shared library.
///
/// Passes through known multiarch subdirectories (e.g. `x86_64-linux-gnu`) so
/// that libraries resolved from `/lib/x86_64-linux-gnu` or
/// `/usr/lib/x86_64-linux-gnu` end up in the corresponding rootfs subdirectory
/// that `LD_LIBRARY_PATH` searches.  Without this the previous `exists()`-based
/// probe always fell back to `usr/lib` on a fresh build, placing multiarch libs
/// outside the dynamic linker's search path.
fn find_lib_dest(lib_path: &Path, lib_dirs: &[PathBuf]) -> PathBuf {
    if let Some(name) = lib_path.file_name() {
        for dir in lib_dirs {
            let candidate = dir.join(name);
            if candidate.exists() || candidate.symlink_metadata().is_ok() {
                return dir.clone();
            }
        }
    }

    const MULTIARCH_SUBDIRS: &[&str] = &["x86_64-linux-gnu", "aarch64-linux-gnu"];
    if let Some(_name) = lib_path.file_name() {
        let lib_path_str = lib_path.to_string_lossy();
        for subdir in MULTIARCH_SUBDIRS {
            let marker = format!("/{subdir}/");
            if let Some(_pos) = lib_path_str.find(&marker) {
                let dest = lib_dirs
                    .iter()
                    .find(|d| d.ends_with(subdir))
                    .cloned()
                    .unwrap_or_else(|| lib_dirs.get(2).cloned().unwrap_or_default());
                let _ = std::fs::create_dir_all(&dest);
                return dest;
            }
        }
    }

    lib_dirs
        .get(2)
        .cloned()
        .unwrap_or_else(|| PathBuf::from("/usr/lib"))
}

/// Embed runtime-specific configuration (php.ini, extensions, etc.).
fn embed_runtime_config(
    interpreter: &str,
    interp_path: &Path,
    rootfs: &Path,
    app_dir: Option<&Path>,
    verbose: bool,
) -> io::Result<usize> {
    let mut count = 0;

    match interpreter {
        "php" => {
            count += embed_php_config(interp_path, rootfs, verbose)?;
            // Install missing PHP extensions from composer.json requirements
            if let Some(dir) = app_dir {
                count += install_missing_php_extensions(dir, interp_path, rootfs, verbose)?;
            }
        }
        "python3" | "python" => {
            count += embed_python_config(interp_path, rootfs, verbose)?;
        }
        "node" => {
            count += embed_node_config(interp_path, rootfs, verbose)?;
        }
        "ruby" => {
            count += embed_ruby_config(interp_path, rootfs, verbose)?;
            count += embed_ruby_native_gems(interp_path, rootfs, verbose)?;
        }
        "perl" => {
            count += embed_perl_config(interp_path, rootfs, verbose)?;
        }
        "java" => {
            count += embed_java_config(interp_path, rootfs, verbose)?;
        }
        _ => {}
    }

    Ok(count)
}

/// Embed PHP configuration: php.ini, extensions directory, and conf.d.
fn embed_php_config(interp_path: &Path, rootfs: &Path, verbose: bool) -> io::Result<usize> {
    let mut count = 0;

    // Find PHP prefix by running `php -r "echo PHP_PREFIX;"`
    let prefix = run_cmd(interp_path, &["-r", "echo PHP_PREFIX;"]);
    if prefix.is_empty() {
        if verbose {
            eprintln!("  embed: could not determine PHP_PREFIX, skipping php.ini");
        }
        return Ok(0);
    }

    let prefix_path = PathBuf::from(&prefix);

    // Determine the target prefix in rootfs
    let target_prefix = rootfs.join("usr").join("local").join("php");

    // Copy php.ini
    let ini_path = prefix_path.join("ini").join("php.ini");
    if ini_path.is_file() {
        let target_ini = target_prefix.join("ini");
        fs::create_dir_all(&target_ini)?;
        fs::copy(&ini_path, target_ini.join("php.ini"))?;
        count += 1;
        if verbose {
            eprintln!("  embed: copied php.ini");
        }
    }

    // Copy conf.d (additional .ini files)
    let conf_d = prefix_path.join("ini").join("conf.d");
    if conf_d.is_dir() {
        let target_conf_d = target_prefix.join("ini").join("conf.d");
        copy_dir_recursive(&conf_d, &target_conf_d)?;
        count += count_dir_files(&target_conf_d);
        if verbose {
            eprintln!(
                "  embed: copied conf.d ({} files)",
                count_dir_files(&target_conf_d)
            );
        }
    }

    // Copy extensions directory
    let ext_dir = prefix_path.join("extensions");
    if ext_dir.is_dir() {
        let target_ext = target_prefix.join("extensions");
        copy_dir_recursive(&ext_dir, &target_ext)?;
        count += count_dir_files(&target_ext);
        if verbose {
            eprintln!(
                "  embed: copied extensions ({} files)",
                count_dir_files(&target_ext)
            );
        }
    }

    // Write a custom php.ini to set correct paths for the embedded environment
    let target_ini = target_prefix.join("ini").join("php.ini");
    if target_ini.is_file() {
        let mut ini_content = fs::read_to_string(&target_ini).unwrap_or_default();
        // Update extension_dir to point to the embedded location
        let new_ext_dir = target_prefix
            .join("extensions")
            .to_string_lossy()
            .to_string();
        ini_content = update_ini_value(&ini_content, "extension_dir", &new_ext_dir);
        // Disable Xdebug by default in embedded mode
        if !ini_content.contains("xdebug.mode") {
            ini_content.push_str("\nxdebug.mode=off\n");
        }
        fs::write(&target_ini, &ini_content)?;
    }

    Ok(count)
}

/// Parse composer.json to extract required PHP extensions (ext-*).
fn parse_composer_extensions(app_dir: &Path) -> Vec<String> {
    let composer_json = app_dir.join("composer.json");
    if !composer_json.is_file() {
        return Vec::new();
    }

    let content = match fs::read_to_string(&composer_json) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut extensions = Vec::new();

    // Check "require" section
    if let Some(require) = json.get("require").and_then(|v| v.as_object()) {
        for (key, _) in require {
            if key.starts_with("ext-") {
                let ext_name = key.strip_prefix("ext-").unwrap_or(key);
                if !PHP_BUILTIN_EXTENSIONS.contains(&ext_name) {
                    extensions.push(ext_name.to_string());
                }
            }
        }
    }

    // Check "require-dev" section
    if let Some(require_dev) = json.get("require-dev").and_then(|v| v.as_object()) {
        for (key, _) in require_dev {
            if key.starts_with("ext-") {
                let ext_name = key.strip_prefix("ext-").unwrap_or(key);
                if !PHP_BUILTIN_EXTENSIONS.contains(&ext_name) {
                    extensions.push(ext_name.to_string());
                }
            }
        }
    }

    extensions.sort();
    extensions.dedup();
    extensions
}

/// Check if a PHP extension is already loaded.
fn is_extension_loaded(interp_path: &Path, ext_name: &str) -> bool {
    let output = Command::new(interp_path)
        .args(["-m"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase())
        .unwrap_or_default();
    // Check both module name and common aliases
    let names = match ext_name {
        "pdo" => vec!["pdo", "pdo_sqlite", "pdo_mysql"],
        "simplexml" => vec!["simplexml", "xmlreader"],
        _ => vec![ext_name],
    };
    for name in names {
        if output.contains(&format!("\n{name}\n")) || output.contains(&format!("[{name}]")) {
            return true;
        }
    }
    false
}

/// Download a PHP binary that includes all common extensions.
///
/// Uses the shivammathur/php-builder GitHub releases which provide
/// pre-compiled PHP binaries with extensions for various platforms.
fn download_php_with_extensions(
    interp_path: &Path,
    rootfs: &Path,
    verbose: bool,
) -> io::Result<usize> {
    let php_major = run_cmd(interp_path, &["-r", "echo PHP_MAJOR_VERSION;"]);
    let php_minor = run_cmd(interp_path, &["-r", "echo PHP_MINOR_VERSION;"]);

    if php_major.is_empty() {
        if verbose {
            eprintln!("  embed: could not determine PHP version");
        }
        return Ok(0);
    }

    let missing = check_missing_extensions(interp_path, verbose);
    if missing.is_empty() {
        return Ok(0);
    }

    let tmp_dir = tempfile::tempdir()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("tempdir failed: {e}")))?;

    // Download and extract
    let extract_dir = match download_and_extract_php(&php_major, &php_minor, &tmp_dir, verbose) {
        Some(d) => d,
        None => {
            if verbose {
                eprintln!("  embed: could not download PHP binary, using system PHP");
            }
            return Ok(0);
        }
    };

    let mut count = 0;
    count += copy_php_binary(&extract_dir, &php_major, &php_minor, rootfs, verbose);
    count += copy_php_libs(&extract_dir, rootfs, verbose);
    count += copy_php_extensions(&extract_dir, rootfs, verbose)?;
    count += copy_php_ini(&extract_dir, rootfs, verbose)?;

    Ok(count)
}

/// Check which common PHP extensions are missing and log them.
fn check_missing_extensions(interp_path: &Path, verbose: bool) -> Vec<&'static str> {
    let common_exts = [
        "bcmath", "calendar", "ctype", "exif", "ftp", "gd", "gettext", "imagick", "intl", "ldap",
        "mysqli", "pcntl", "redis", "shmop", "soap", "sockets", "xsl", "zip",
    ];

    let missing: Vec<&str> = common_exts
        .iter()
        .filter(|ext| !is_extension_loaded(interp_path, ext))
        .copied()
        .collect();

    if missing.is_empty() {
        if verbose {
            eprintln!("  embed: all common PHP extensions already available");
        }
    } else if verbose {
        eprintln!("  embed: missing PHP extensions: {}", missing.join(", "));
        eprintln!("  embed: downloading PHP binary with extensions...");
    }

    missing
}

/// Download and extract the PHP archive. Returns the extract directory on success.
fn download_and_extract_php(
    php_major: &str,
    php_minor: &str,
    tmp_dir: &tempfile::TempDir,
    verbose: bool,
) -> Option<PathBuf> {
    let php_tar = tmp_dir
        .path()
        .join(format!("php-{php_major}.{php_minor}.tar.xz"));
    let distro = detect_linux_distro();

    let download_url = format!(
        "https://github.com/shivammathur/php-builder/releases/download/{php_major}.{php_minor}/php_{php_major}.{php_minor}%2B{distro}.tar.xz"
    );

    if verbose {
        eprintln!("  embed: downloading from {download_url}");
    }

    let ok = Command::new("curl")
        .args(["-fsSL", "-o", &php_tar.to_string_lossy(), &download_url])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !ok {
        return None;
    }
    if verbose {
        eprintln!("  embed: downloaded PHP binary");
    }

    let extract_dir = tmp_dir.path().join("php");
    fs::create_dir_all(&extract_dir).ok()?;

    let extracted = Command::new("tar")
        .args([
            "xJf",
            &php_tar.to_string_lossy(),
            "-C",
            &extract_dir.to_string_lossy(),
            "--strip-components=1",
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !extracted {
        return None;
    }

    Some(extract_dir)
}

/// Copy the PHP binary from the extracted archive into rootfs.
fn copy_php_binary(
    extract_dir: &Path,
    php_major: &str,
    php_minor: &str,
    rootfs: &Path,
    verbose: bool,
) -> usize {
    let candidates = [
        extract_dir.join("usr").join("bin").join("php"),
        extract_dir
            .join("usr")
            .join("bin")
            .join(format!("php{php_major}.{php_minor}")),
        extract_dir.join("bin").join("php"),
    ];

    let php_bin = match candidates.iter().find(|p| p.is_file()) {
        Some(p) => p,
        None => return 0,
    };

    let target = rootfs.join("usr").join("bin").join("php");
    if fs::copy(php_bin, &target).is_err() {
        return 0;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&target, fs::Permissions::from_mode(0o755));
    }

    if verbose {
        eprintln!("  embed: replaced PHP binary with version that includes extensions");
    }
    1
}

/// Copy shared libraries bundled in the PHP archive into rootfs.
fn copy_php_libs(extract_dir: &Path, rootfs: &Path, verbose: bool) -> usize {
    let mut count = 0;
    let lib_dirs = [
        extract_dir.join("usr/lib/x86_64-linux-gnu"),
        extract_dir.join("usr/lib"),
        extract_dir.join("lib/x86_64-linux-gnu"),
        extract_dir.join("lib"),
    ];

    let target_lib = rootfs.join("usr/lib/x86_64-linux-gnu");
    if fs::create_dir_all(&target_lib).is_err() {
        return 0;
    }

    for lib_dir in &lib_dirs {
        let dir = match fs::read_dir(lib_dir) {
            Ok(d) => d,
            Err(_) => continue,
        };
        for entry in dir.flatten() {
            let src = entry.path();
            if !src.is_file() {
                continue;
            }
            let dst = target_lib.join(entry.file_name());
            if !dst.exists() && fs::copy(&src, &dst).is_ok() {
                count += 1;
            }
        }
        if verbose && count > 0 {
            eprintln!("  embed: copied shared libs from {}", lib_dir.display());
        }
    }
    count
}

/// Copy PHP extensions from the extracted archive into rootfs.
fn copy_php_extensions(extract_dir: &Path, rootfs: &Path, verbose: bool) -> io::Result<usize> {
    let ext_candidates = [
        extract_dir.join("lib/php/extensions"),
        extract_dir.join("usr/lib/php/20240924"),
        extract_dir.join("usr/lib/php/20240814"),
    ];

    for ext_dir in &ext_candidates {
        if !ext_dir.is_dir() {
            if verbose {
                eprintln!(
                    "  embed: checking ext dir: {} (exists=false)",
                    ext_dir.display()
                );
            }
            continue;
        }
        if verbose {
            eprintln!(
                "  embed: checking ext dir: {} (exists=true)",
                ext_dir.display()
            );
        }

        let target_ext = rootfs.join("usr/local/php/extensions");
        copy_dir_recursive(ext_dir, &target_ext)?;
        let n = count_dir_files(&target_ext);
        if verbose {
            eprintln!("  embed: copied {n} extension files");
        }
        return Ok(n);
    }
    Ok(0)
}

/// Copy php.ini from the extracted archive into rootfs, if present.
fn copy_php_ini(extract_dir: &Path, rootfs: &Path, verbose: bool) -> io::Result<usize> {
    let ini_candidates = [
        extract_dir.join("etc/php.ini"),
        extract_dir.join("usr/local/etc/php.ini"),
    ];

    for ini_file in &ini_candidates {
        if ini_file.is_file() {
            let target_ini = rootfs.join("usr/local/php/ini");
            fs::create_dir_all(&target_ini)?;
            fs::copy(ini_file, target_ini.join("php.ini"))?;
            if verbose {
                eprintln!("  embed: copied php.ini with extensions configured");
            }
            return Ok(1);
        }
    }
    Ok(0)
}

/// Detect Linux distribution for PHP download URL.
fn detect_linux_distro() -> &'static str {
    // Try to detect from /etc/os-release
    if let Ok(content) = fs::read_to_string("/etc/os-release") {
        let content_lower = content.to_lowercase();
        // Check for Ubuntu first — Ubuntu's /etc/os-release contains
        // `ID_LIKE=debian`, so a naive `contains("debian")` check would
        // misidentify Ubuntu as Debian.
        if content_lower.contains("ubuntu") {
            if content_lower.contains("24.04") || content_lower.contains("noble") {
                return "ubuntu24.04";
            }
            if content_lower.contains("22.04") || content_lower.contains("jammy") {
                return "ubuntu22.04";
            }
            return "ubuntu24.04";
        }
        if content_lower.contains("debian") {
            if content_lower.contains("13") {
                return "debian13";
            }
            if content_lower.contains("12") {
                return "debian12";
            }
            return "debian11";
        }
    }

    // Fallback to Debian 12
    "debian12"
}

/// Install missing PHP extensions by downloading them from PECL.
fn install_missing_php_extensions(
    app_dir: &Path,
    interp_path: &Path,
    rootfs: &Path,
    verbose: bool,
) -> io::Result<usize> {
    let mut count = 0;

    // Parse required extensions from composer.json
    let required_exts = parse_composer_extensions(app_dir);
    if required_exts.is_empty() {
        if verbose {
            eprintln!("  embed: no additional PHP extensions required");
        }
        return Ok(0);
    }

    if verbose {
        eprintln!(
            "  embed: required PHP extensions: {}",
            required_exts.join(", ")
        );
    }

    // Get the target extension directory in rootfs
    let target_ext_dir = rootfs
        .join("usr")
        .join("local")
        .join("php")
        .join("extensions");
    fs::create_dir_all(&target_ext_dir)?;

    // Get the target conf.d directory
    let target_conf_d = rootfs
        .join("usr")
        .join("local")
        .join("php")
        .join("ini")
        .join("conf.d");
    fs::create_dir_all(&target_conf_d)?;

    // First, try to download a PHP binary with all extensions
    // This is more reliable than downloading individual extensions
    let php_with_exts = download_php_with_extensions(interp_path, rootfs, verbose)?;
    count += php_with_exts;

    // If we got a new PHP binary, use it for further extension checks
    let effective_interp =
        if php_with_exts > 0 && rootfs.join("usr").join("bin").join("php").is_file() {
            rootfs.join("usr").join("bin").join("php")
        } else {
            interp_path.to_path_buf()
        };

    for ext_name in &required_exts {
        // Skip if already loaded (check against the effective PHP binary)
        if is_extension_loaded(&effective_interp, ext_name) {
            if verbose {
                eprintln!("  embed: extension {ext_name} already loaded, skipping");
            }
            continue;
        }

        // Check if the extension .so file exists in the rootfs extensions directory
        let target_ext = rootfs
            .join("usr")
            .join("local")
            .join("php")
            .join("extensions")
            .join(format!("{ext_name}.so"));
        if target_ext.exists() {
            if verbose {
                eprintln!("  embed: extension {ext_name}.so found in rootfs, enabling");
            }

            // Create a conf.d entry to enable the extension
            let target_conf_d = rootfs
                .join("usr")
                .join("local")
                .join("php")
                .join("ini")
                .join("conf.d");
            fs::create_dir_all(&target_conf_d)?;
            let ini_content = format!("extension={ext_name}\n");
            let ini_path = target_conf_d.join(format!("{ext_name}.ini"));
            fs::write(&ini_path, &ini_content)?;
            count += 1;
            continue;
        }

        // Extension not found anywhere - warn but continue
        if verbose {
            eprintln!("  embed: warning: extension {ext_name} not found, app may fail");
        }
    }

    Ok(count)
}

/// Embed Python configuration (standard library modules).
fn embed_python_config(interp_path: &Path, rootfs: &Path, verbose: bool) -> io::Result<usize> {
    let mut count = 0;

    // Find Python stdlib path using sysconfig (correct version-tagged directory)
    let stdlib = run_cmd(
        interp_path,
        &[
            "-c",
            "import sysconfig; print(sysconfig.get_path('stdlib'))",
        ],
    );
    if stdlib.is_empty() {
        return Ok(0);
    }

    let stdlib_path = PathBuf::from(&stdlib);
    if stdlib_path.is_dir() {
        let target = rootfs
            .join("usr")
            .join("lib")
            .join(stdlib_path.file_name().unwrap_or(stdlib_path.as_os_str()));
        copy_dir_recursive(&stdlib_path, &target)?;
        count += count_dir_files(&target);
        if verbose {
            eprintln!(
                "  embed: copied Python stdlib ({} files)",
                count_dir_files(&target)
            );
        }
    }

    Ok(count)
}

/// Embed Node.js configuration (`node_modules`, etc.).
fn embed_node_config(_interp_path: &Path, _rootfs: &Path, _verbose: bool) -> io::Result<usize> {
    // Node.js typically doesn't need special config embedding
    // The app's node_modules are already copied with the app files
    Ok(0)
}

/// Embed Ruby configuration: stdlib + gems.
fn embed_ruby_config(interp_path: &Path, rootfs: &Path, verbose: bool) -> io::Result<usize> {
    let mut count = 0;

    let rubylibdir = run_cmd(interp_path, &["-e", "print RbConfig::CONFIG['rubylibdir']"]);
    let gemdir = run_cmd(interp_path, &["-e", "print Gem.dir"]);

    if !rubylibdir.is_empty() {
        let src = PathBuf::from(&rubylibdir);
        if src.is_dir() {
            let rel = src.strip_prefix("/").unwrap_or(&src);
            let dst = rootfs.join(rel);
            copy_dir_recursive(&src, &dst)?;
            count += count_dir_files(&dst);
            if verbose {
                eprintln!("  embed: Ruby stdlib ({} files)", count_dir_files(&dst));
            }
        }
    }

    if !gemdir.is_empty() {
        let src = PathBuf::from(&gemdir);
        if src.is_dir() {
            let rel = src.strip_prefix("/").unwrap_or(&src);
            let dst = rootfs.join(rel);
            copy_dir_recursive(&src, &dst)?;
            let c = count_dir_files(&dst);
            count += c;
            if verbose {
                eprintln!("  embed: Ruby gems ({} files)", c);
            }
        }
    }

    Ok(count)
}

/// Scan Ruby gem directories for .so native extensions, run ldd on each,
/// and embed their shared library dependencies into the rootfs.
fn embed_ruby_native_gems(interp_path: &Path, rootfs: &Path, verbose: bool) -> io::Result<usize> {
    let gemdir = run_cmd(interp_path, &["-e", "print Gem.dir"]);
    if gemdir.is_empty() {
        return Ok(0);
    }
    let gem_path = PathBuf::from(&gemdir);
    if !gem_path.is_dir() {
        return Ok(0);
    }

    let mut seen = HashSet::new();
    let lib_dirs = resolve_lib_dirs(rootfs);
    let mut count = 0;
    let mut stack = vec![gem_path];

    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|s| s.to_str()) != Some("so") {
                continue;
            }

            if verbose {
                eprintln!("  embed: Ruby native gem {}", path.display());
            }

            let deps = ldd_deps(&path).unwrap_or_default();

            for lib_path in &deps {
                let name = match lib_path.file_name() {
                    Some(n) => n,
                    None => continue,
                };
                let name_str = name.to_string_lossy();
                if !seen.insert(name_str.to_string()) {
                    continue;
                }
                let dest_dir = find_lib_dest(lib_path, &lib_dirs);
                fs::create_dir_all(&dest_dir)?;
                let dest_lib = dest_dir.join(name);
                let _ = fs::remove_file(&dest_lib);
                fs::copy(lib_path, &dest_lib)?;
                count += 1;

                if verbose {
                    eprintln!(
                        "  embed: ruby gem lib {} -> {}",
                        name_str,
                        dest_dir.display()
                    );
                }
            }
        }
    }

    Ok(count)
}

/// Embed Perl configuration: @INC directories.
fn embed_perl_config(interp_path: &Path, rootfs: &Path, verbose: bool) -> io::Result<usize> {
    let mut count = 0;

    let privlib = run_cmd(
        interp_path,
        &["-MConfig", "-e", "print $Config{privlibexp}"],
    );
    let archlib = run_cmd(
        interp_path,
        &["-MConfig", "-e", "print $Config{archlibexp}"],
    );

    for path_str in [&privlib, &archlib] {
        if path_str.is_empty() {
            continue;
        }
        let src = PathBuf::from(path_str.as_str());
        if src.is_dir() {
            let rel = src.strip_prefix("/").unwrap_or(&src);
            let dst = rootfs.join(rel);
            copy_dir_recursive(&src, &dst)?;
            let c = count_dir_files(&dst);
            count += c;
            if verbose {
                eprintln!("  embed: Perl lib ({} files)", c);
            }
        }
    }

    Ok(count)
}

/// Embed Java JRE: find java.home and copy lib/ (contains rt.jar / modules).
fn embed_java_config(interp_path: &Path, rootfs: &Path, verbose: bool) -> io::Result<usize> {
    let mut count = 0;

    let java_home_output = std::process::Command::new(interp_path)
        .args(["-XshowSettings:properties", "-version"])
        .output()
        .ok();
    let stderr = java_home_output
        .as_ref()
        .map(|o| String::from_utf8_lossy(&o.stderr).to_string())
        .unwrap_or_default();
    let home = stderr
        .lines()
        .find_map(|line| line.trim().strip_prefix("java.home = "))
        .map(|s| s.trim().to_string());

    let Some(java_home) = home else {
        if verbose {
            eprintln!("  embed: could not determine JAVA_HOME, skipping JRE");
        }
        return Ok(0);
    };

    let java_home_path = PathBuf::from(&java_home);

    // JDK 9+ uses lib/modules (jlink); older uses jre/lib/rt.jar
    let lib_src = java_home_path.join("lib");
    if lib_src.is_dir() {
        let dst_stable = rootfs.join("usr/lib/jvm/java");
        copy_dir_recursive(&lib_src, &dst_stable.join("lib"))?;
        count += count_dir_files(&dst_stable.join("lib"));
        if verbose {
            eprintln!(
                "  embed: Java JRE lib ({} files) from {}",
                count_dir_files(&dst_stable.join("lib")),
                lib_src.display()
            );
        }
    }

    // Copy bin/ (java, javac, etc.) into a stable JVM path
    let bin_src = java_home_path.join("bin");
    if bin_src.is_dir() {
        let dst_stable = rootfs.join("usr/lib/jvm/java/bin");
        copy_dir_recursive(&bin_src, &dst_stable)?;
        count += count_dir_files(&dst_stable);
        #[cfg(unix)]
        {
            let _ = std::fs::remove_file(rootfs.join("usr/bin/java"));
            let _ = std::fs::create_dir_all(rootfs.join("usr/bin"));
            let _ = std::os::unix::fs::symlink(
                "/usr/lib/jvm/java/bin/java",
                rootfs.join("usr/bin/java"),
            );
        }
    }

    // ldd on `java` typically misses libjvm.so (loaded via dlopen).
    // Find and copy it explicitly.
    let jvm_lib = java_home_path.join("lib/server/libjvm.so");
    if jvm_lib.is_file() {
        let dst = rootfs.join("usr/lib/jvm/java/lib/server/libjvm.so");
        if let Some(parent) = dst.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::copy(&jvm_lib, &dst);
        count += 1;
    }
    // Also try the jre/ variant (JDK ≤ 8)
    let jvm_lib_jre = java_home_path.join("jre/lib/amd64/server/libjvm.so");
    if jvm_lib_jre.is_file() {
        let dst = rootfs.join("usr/lib/jvm/java/jre/lib/amd64/server/libjvm.so");
        if let Some(parent) = dst.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::copy(&jvm_lib_jre, &dst);
        count += 1;
    }

    Ok(count)
}

/// Run a command and capture stdout.
fn run_cmd(program: &Path, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Update a php.ini-style key=value line.
fn update_ini_value(content: &str, key: &str, value: &str) -> String {
    let mut result = Vec::new();
    let mut found = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(';') || trimmed.starts_with('[') {
            result.push(line.to_string());
            continue;
        }
        if let Some(idx) = trimmed.find('=') {
            let k = trimmed[..idx].trim();
            if k == key {
                result.push(format!("{key} = \"{value}\""));
                found = true;
                continue;
            }
        }
        result.push(line.to_string());
    }
    if !found {
        result.push(format!("{key} = \"{value}\""));
    }
    result.join("\n")
}

/// Recursively copy a directory.
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Describe which files to keep when copying a runtime's isolated install
/// directory into the rootfs. Runtimes differ in what they need at runtime
/// (DLLs, stdlib, `node_modules`) and in what they carry that is dead weight
/// (debug symbols, sources, docs).
#[derive(Clone, Copy)]
pub struct RuntimeProfile {
    /// When set, copy only entries whose relative path matches one of these
    /// patterns. Each pattern is either a whole relative path (e.g. `Lib`,
    /// `bin/npm.cmd`) or an extension glob (`*.dll`, `*.exe`).
    pub only: Option<&'static [&'static str]>,
    /// Skip any entry whose relative path or file extension matches one of
    /// these patterns, applied after `only`.
    pub exclude: &'static [&'static str],
}

impl RuntimeProfile {
    /// A runtime whose single binary is fully self-contained (V8/Go/Rust-like)
    /// and needs no additional install-tree files.
    pub const fn self_contained() -> Self {
        RuntimeProfile {
            only: None,
            exclude: &[],
        }
    }
}

/// Copy an isolated runtime install directory into `rootfs/usr/`, honouring a
/// [`RuntimeProfile`]. The install tree is laid out as shipped (bin at
/// `usr/bin`, DLLs/stdlib beside it) so the runtime finds its own files via
/// its standard relative layout. Returns the number of files copied.
pub fn embed_runtime_dir(
    install_dir: &Path,
    profile: &RuntimeProfile,
    rootfs: &Path,
    verbose: bool,
) -> io::Result<usize> {
    let dest_root = rootfs.join("usr");
    fs::create_dir_all(&dest_root)?;
    let mut count = 0;
    copy_runtime_filtered(install_dir, &dest_root, profile, "", &mut count)?;
    if verbose {
        eprintln!("  embed: copied runtime install tree ({} files)", count);
    }
    Ok(count)
}

/// Profile for an isolated Python install: keep the interpreter, DLLs and the
/// stdlib, drop debug symbols, headers and dev scaffolding.
pub const fn python_runtime_profile() -> &'static RuntimeProfile {
    static PROFILE: RuntimeProfile = RuntimeProfile {
        only: None,
        exclude: &[
            "*.pdb",
            "*.pyc",
            "*.pyo",
            "include",
            "libs",
            "Scripts",
            "Tools",
            "Doc",
            "tcl",
            "LICENSE.txt",
        ],
    };
    &PROFILE
}

/// Profile for a Node install: the binary is self-contained, but the runtime
/// ships npm scaffolding beside it that apps may firewall through.
pub const fn node_runtime_profile() -> &'static RuntimeProfile {
    static PROFILE: RuntimeProfile = RuntimeProfile {
        only: None,
        exclude: &["*.pdb"],
    };
    &PROFILE
}

/// Profile for an Electron install: the V8/Chromium runtime plus its DLLs and
/// `resources/` payload, minus debug symbols.
pub const fn electron_runtime_profile() -> &'static RuntimeProfile {
    static PROFILE: RuntimeProfile = RuntimeProfile {
        only: None,
        exclude: &["*.pdb"],
    };
    &PROFILE
}

fn copy_runtime_filtered(
    src: &Path,
    dst: &Path,
    profile: &RuntimeProfile,
    rel: &str,
    count: &mut usize,
) -> io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let child_rel = if rel.is_empty() {
            name_str.to_string()
        } else {
            format!("{rel}/{name_str}")
        };

        if let Some(only) = profile.only {
            if !matches_pattern(&child_rel, only) {
                continue;
            }
        }
        if matches_pattern(&child_rel, profile.exclude) {
            continue;
        }

        let src_path = entry.path();
        let dst_path = dst.join(&name);
        if src_path.is_dir() {
            fs::create_dir_all(&dst_path)?;
            copy_runtime_filtered(&src_path, &dst_path, profile, &child_rel, count)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
            *count += 1;
        }
    }
    Ok(())
}

/// Match a relative path against a list of patterns. A pattern either equals
/// the whole path, equals a leading path segment (so `Lib` covers `Lib/sub`),
/// or is an extension glob `*.ext` matching the file's extension.
fn matches_pattern(rel: &str, patterns: &[&str]) -> bool {
    let matches_ext = rel
        .rsplit('.')
        .next()
        .map(|ext| patterns.iter().any(|p| p == &format!("*.{ext}")))
        .unwrap_or(false);
    if matches_ext {
        return true;
    }
    patterns
        .iter()
        .any(|p| rel == *p || rel.starts_with(&format!("{p}/")))
}

/// Scan `rootfs/app/node_modules/` for `.node` files (N-API native addons),
/// run `ldd` on each, and embed their shared library dependencies into the rootfs.
pub fn embed_napi_addons(rootfs: &Path, verbose: bool) -> io::Result<usize> {
    let node_modules = rootfs.join("app/node_modules");
    if !node_modules.is_dir() {
        return Ok(0);
    }

    let mut seen = HashSet::new();
    let lib_dirs = resolve_lib_dirs(rootfs);
    let mut count = 0;

    let mut stack = vec![node_modules.clone()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|s| s.to_str()) != Some("node") {
                continue;
            }

            if verbose {
                eprintln!("  embed: N-API addon {}", path.display());
            }

            let deps = match ldd_deps(&path) {
                Ok(d) => d,
                Err(e) => {
                    if verbose {
                        eprintln!("  embed: ldd failed for {}: {e}", path.display());
                    }
                    Vec::new()
                }
            };

            for lib_path in &deps {
                let name = match lib_path.file_name() {
                    Some(n) => n,
                    None => continue,
                };
                let name_str = name.to_string_lossy();
                if !seen.insert(name_str.to_string()) {
                    continue;
                }

                let dest_dir = find_lib_dest(lib_path, &lib_dirs);
                fs::create_dir_all(&dest_dir)?;
                let dest_lib = dest_dir.join(name);
                let _ = fs::remove_file(&dest_lib);
                fs::copy(lib_path, &dest_lib)?;
                count += 1;

                if verbose {
                    eprintln!("  embed: napi lib {} -> {}", name_str, dest_dir.display());
                }
            }
        }
    }

    Ok(count)
}

/// Count files in a directory recursively.
fn count_dir_files(dir: &Path) -> usize {
    let mut count = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                count += count_dir_files(&entry.path());
            } else {
                count += 1;
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    /// `parse_loader_line_glibc` - parse loader line glibc.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn parse_loader_line_glibc() {
        let line = "/lib64/ld-linux-x86-64.so.2 (0x00007f1234567890)";
        assert_eq!(
            parse_loader_line(line),
            Some(PathBuf::from("/lib64/ld-linux-x86-64.so.2"))
        );
    }

    #[test]
    #[cfg(unix)]
    /// `parse_loader_line_musl` - parse loader line musl.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn parse_loader_line_musl() {
        let line = "/lib/ld-musl-x86_64.so.1 (0x7f...)";
        assert_eq!(
            parse_loader_line(line),
            Some(PathBuf::from("/lib/ld-musl-x86_64.so.1"))
        );
    }

    #[test]
    #[cfg(unix)]
    /// `parse_loader_line_rejects_regular_deps` - parse loader line rejects regular deps.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn parse_loader_line_rejects_regular_deps() {
        assert_eq!(
            parse_loader_line("libm.so.6 => /lib/x86_64-linux-gnu/libm.so.6 (0x...)"),
            None
        );
        assert_eq!(parse_loader_line("linux-vdso.so.1 (0x...)"), None);
        assert_eq!(parse_loader_line("not found"), None);
    }

    #[test]
    /// `update_ini_value_existing` - update ini value existing.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn update_ini_value_existing() {
        let content = "memory_limit = 128M\nupload_max_filesize = 2M\n";
        let result = update_ini_value(content, "memory_limit", "/new/path");
        assert!(result.contains("memory_limit = \"/new/path\""));
        assert!(result.contains("upload_max_filesize = 2M"));
    }

    #[test]
    /// `update_ini_value_missing` - update ini value missing.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn update_ini_value_missing() {
        let content = "memory_limit = 128M\n";
        let result = update_ini_value(content, "extension_dir", "/ext");
        assert!(result.contains("extension_dir = \"/ext\""));
    }

    #[test]
    /// `count_dir_files_empty` - count dir files empty.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn count_dir_files_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(count_dir_files(dir.path()), 0);
    }

    #[test]
    /// `count_dir_files_nested` - count dir files nested.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn count_dir_files_nested() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("b.txt"), "b").unwrap();
        assert_eq!(count_dir_files(dir.path()), 2);
    }
}
