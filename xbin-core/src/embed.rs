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

/// Parse `ldd` output to find shared library paths for a binary or .node file.
pub(crate) fn ldd_deps(interp_path: &Path) -> io::Result<Vec<PathBuf>> {
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
        "ruby" => {
            count += embed_ruby_config(interp_path, rootfs, verbose)?;
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
        let _ = std::fs::create_dir_all(dst.parent().unwrap());
        let _ = std::fs::copy(&jvm_lib, &dst);
        count += 1;
    }
    // Also try the jre/ variant (JDK ≤ 8)
    let jvm_lib_jre = java_home_path.join("jre/lib/amd64/server/libjvm.so");
    if jvm_lib_jre.is_file() {
        let dst = rootfs.join("usr/lib/jvm/java/jre/lib/amd64/server/libjvm.so");
        let _ = std::fs::create_dir_all(dst.parent().unwrap());
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
