use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Embed an interpreter and its shared library dependencies into a rootfs.
///
/// Returns the number of files copied (interpreter + shared libs + config).
pub fn embed_interpreter(interpreter: &str, rootfs: &Path, verbose: bool) -> io::Result<usize> {
    let interp_path = find_interpreter(interpreter).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("interpreter '{interpreter}' not found on PATH"),
        )
    })?;

    if verbose {
        eprintln!("  embed: interpreter found at {}", interp_path.display());
    }

    let mut count = 0;

    // Copy the interpreter binary to rootfs/usr/bin/
    let bin_dir = rootfs.join("usr/bin");
    fs::create_dir_all(&bin_dir)?;
    let dest_bin = bin_dir.join(interpreter);
    fs::copy(&interp_path, &dest_bin)?;
    count += 1;

    // Set executable permission
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dest_bin, fs::Permissions::from_mode(0o755))?;
    }

    if verbose {
        eprintln!("  embed: copied {interpreter} to {}", dest_bin.display());
    }

    // Find and copy shared library dependencies via ldd
    let deps = ldd_deps(&interp_path)?;
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

        // Copy the symlink target if it's a symlink, otherwise copy the file
        if lib_path.is_symlink() {
            let target = fs::read_link(lib_path)?;
            // Create symlink in rootfs
            if dest_lib.exists() || dest_lib.symlink_metadata().is_ok() {
                let _ = fs::remove_file(&dest_lib);
            }
            std::os::unix::fs::symlink(&target, &dest_lib)?;
        } else {
            fs::copy(lib_path, &dest_lib)?;
        }
        count += 1;

        if verbose {
            eprintln!("  embed: lib {} -> {}", name_str, dest_dir.display());
        }
    }

    // Copy runtime-specific config (php.ini, extensions, etc.)
    count += embed_runtime_config(interpreter, &interp_path, rootfs, verbose)?;

    Ok(count)
}

/// Find the interpreter binary on the system PATH.
fn find_interpreter(name: &str) -> Option<PathBuf> {
    let output = Command::new("which").arg(name).output().ok()?;
    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    None
}

/// Parse `ldd` output to find shared library paths.
fn ldd_deps(interp_path: &Path) -> io::Result<Vec<PathBuf>> {
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
        // Skip empty lines and linux-vdso/ld-linux
        if line.is_empty() || line.starts_with("linux-vdso") || line.starts_with("ld-linux") {
            continue;
        }
        // Parse: "libfoo.so.1 => /lib/x86_64-linux-gnu/libfoo.so.1 (0x...)"
        if let Some(path_str) = line.split("=>").nth(1) {
            let path_str = path_str.trim();
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
    }

    Ok(deps)
}

/// Resolve standard library directories in the rootfs.
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
fn find_lib_dest(lib_path: &Path, lib_dirs: &[PathBuf]) -> PathBuf {
    // If the source is in a known lib dir, mirror the structure
    if let Some(name) = lib_path.file_name() {
        for dir in lib_dirs {
            let candidate = dir.join(name);
            if candidate.exists() || candidate.symlink_metadata().is_ok() {
                return dir.clone();
            }
        }
    }
    // Fallback: use usr/lib
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
    verbose: bool,
) -> io::Result<usize> {
    let mut count = 0;

    match interpreter {
        "php" => {
            count += embed_php_config(interp_path, rootfs, verbose)?;
        }
        "python3" | "python" => {
            count += embed_python_config(interp_path, rootfs, verbose)?;
        }
        "node" => {
            count += embed_node_config(interp_path, rootfs, verbose)?;
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

/// Embed Python configuration (standard library modules).
fn embed_python_config(interp_path: &Path, rootfs: &Path, verbose: bool) -> io::Result<usize> {
    let mut count = 0;

    // Find Python stdlib path
    let stdlib = run_cmd(interp_path, &["-c", "import sys; print(sys.prefix)"]);
    if stdlib.is_empty() {
        return Ok(0);
    }

    let stdlib_path = PathBuf::from(&stdlib).join("lib").join("python3");
    if stdlib_path.is_dir() {
        let target = rootfs.join("usr").join("lib").join("python3");
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
fn copy_dir_recursive(src: &Path, dst: &Path) -> io::Result<()> {
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
    fn update_ini_value_existing() {
        let content = "memory_limit = 128M\nupload_max_filesize = 2M\n";
        let result = update_ini_value(content, "memory_limit", "/new/path");
        assert!(result.contains("memory_limit = \"/new/path\""));
        assert!(result.contains("upload_max_filesize = 2M"));
    }

    #[test]
    fn update_ini_value_missing() {
        let content = "memory_limit = 128M\n";
        let result = update_ini_value(content, "extension_dir", "/ext");
        assert!(result.contains("extension_dir = \"/ext\""));
    }

    #[test]
    fn count_dir_files_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(count_dir_files(dir.path()), 0);
    }

    #[test]
    fn count_dir_files_nested() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("b.txt"), "b").unwrap();
        assert_eq!(count_dir_files(dir.path()), 2);
    }
}
