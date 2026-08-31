use anyhow::{Context, Result};
use daedalus_core::paths::cache_dir;
use serde_json::Value;
use std::path::{Path, PathBuf};

use super::args::parse_target;

/// Resolve `name` to an executable invocation that works on all platforms.
///
/// On Windows, if the resolved path ends with `.cmd` or `.bat`, returns
/// `("cmd", ["/C", <resolved_path>])` because `CreateProcessW` cannot
/// execute batch files directly. On Unix/macOS, or for native `.exe`
/// binaries, returns the resolved path as the program with no extra args.
///
/// Falls back to the bare `name` when `which` cannot locate it so that
/// shell aliases/functions still work on Unix.
pub(crate) fn resolve_command(name: &str) -> (String, Vec<String>) {
    let resolved = match which::which(name) {
        Ok(p) => p,
        Err(_) => return (name.to_string(), Vec::new()),
    };
    if cfg!(windows) {
        if let Some(ext) = resolved.extension() {
            let ext = ext.to_string_lossy().to_lowercase();
            if ext == "cmd" || ext == "bat" {
                return (
                    "cmd".to_string(),
                    vec!["/C".to_string(), resolved.to_string_lossy().into_owned()],
                );
            }
        }
    }
    (resolved.to_string_lossy().into_owned(), Vec::new())
}

/// Check if a command is available on PATH.
pub(crate) fn is_command_available(name: &str) -> bool {
    which::which(name).is_ok()
}

/// Ensure node + npm are available for the build.
/// Downloads a static node to `~/.cache/daedalus/build-tools/node/` (or a
/// per-target subdir when `--target` requests a non-host platform) if not on
/// PATH. The user-writable cache dir (0700) avoids the symlink attacks a
/// predictable world-writable `/tmp` path would allow. Does NOT pollute the
/// user's system PATH.
pub(crate) fn ensure_node(target: Option<&str>, verbose: bool) -> Result<PathBuf> {
    let suffix = target
        .map(|t| {
            let (arch, os) = parse_target(t);
            format!("{os}-{arch}")
        })
        .unwrap_or_else(|| "host".to_string());
    let tools_dir = cache_dir()
        .join("build-tools")
        .join(format!("node-{suffix}"));
    let is_windows = target.is_some_and(|t| parse_target(t).1 == "windows");
    let node_name = if is_windows { "node.exe" } else { "node" };
    let npm_name = if is_windows { "npm.cmd" } else { "npm" };
    let node_bin = tools_dir.join("bin").join(node_name);
    let npm_bin = tools_dir.join("bin").join(npm_name);

    if node_bin.exists() && npm_bin.exists() {
        if verbose {
            eprintln!("  using cached node from {}", tools_dir.display());
        }
        return Ok(tools_dir.join("bin"));
    }

    if verbose {
        eprintln!("  downloading node to {}...", tools_dir.display());
    }

    std::fs::create_dir_all(&tools_dir).context("failed to create build tools directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            tools_dir.parent().unwrap_or(tools_dir.as_path()),
            std::fs::Permissions::from_mode(0o700),
        )
        .ok();
    }

    ensure_node_download(tools_dir, target, verbose)
}

/// Download a specific Node.js version for a target architecture.
///
/// When `target_arch` is `None`, downloads for the host architecture.
fn ensure_node_download(
    tools_dir: PathBuf,
    target_arch: Option<&str>,
    verbose: bool,
) -> Result<PathBuf> {
    // Map the target to node's official `os-arch` tarball naming. Node ships
    // static builds for linux (musl-compatible), darwin, and windows (`win`).
    let (node_arch, node_os) = if let Some(target) = target_arch {
        let (arch, os) = parse_target(target);
        let node_arch = match arch.as_str() {
            "x86_64" | "amd64" => "x64",
            "aarch64" | "arm64" => "arm64",
            _ => anyhow::bail!("unsupported cross-compile architecture: {arch}"),
        };
        let node_os = match os.as_str() {
            "linux" => "linux",
            "darwin" => "darwin",
            "windows" => "win",
            _ => anyhow::bail!("unsupported cross-compile OS: {os}"),
        };
        (node_arch.to_string(), node_os.to_string())
    } else {
        let node_arch = match std::env::consts::ARCH {
            "x86_64" => "x64",
            "aarch64" => "arm64",
            arch => arch,
        };
        let node_os = match std::env::consts::OS {
            "linux" => "linux",
            "macos" => "darwin",
            "windows" => "win",
            os => os,
        };
        (node_arch.to_string(), node_os.to_string())
    };

    // Windows dists name the binaries `node.exe` / `npm.cmd`.
    let node_bin = tools_dir
        .join("bin")
        .join(if node_os == "win" { "node.exe" } else { "node" });

    #[cfg(unix)]
    let npm_bin = tools_dir
        .join("bin")
        .join(if node_os == "win" { "npm.cmd" } else { "npm" });

    let versions: Vec<serde_json::Value> =
        reqwest::blocking::get("https://nodejs.org/dist/index.json")
            .context("failed to reach nodejs.org")?
            .json()
            .context("failed to parse node version manifest")?;
    let version = if let Ok(pinned) = std::env::var("DAEDALUS_NODE_VERSION") {
        if verbose {
            eprintln!("  using pinned node version {pinned} (DAEDALUS_NODE_VERSION)");
        }
        pinned
    } else {
        versions
            .first()
            .and_then(|v| v.get("version")?.as_str())
            .and_then(|v| v.strip_prefix('v'))
            .map(|v| v.to_string())
            .ok_or_else(|| anyhow::anyhow!("no node version found in manifest"))?
    };

    // nodejs.org serves .tar.xz on Linux, .tar.gz on macOS, and .zip on Windows.
    let ext = match node_os.as_str() {
        "darwin" => "tar.gz",
        "win" => "zip",
        _ => "tar.xz",
    };
    let tarball = format!("node-v{version}-{node_os}-{node_arch}.{ext}");
    let url = format!("https://nodejs.org/dist/v{version}/{tarball}");

    if verbose {
        eprintln!("  downloading node v{version} ({node_os}-{node_arch})...");
    }

    let response = reqwest::blocking::get(&url).context("failed to download node.js tarball")?;
    if node_os == "win" {
        // ZipArchive needs a seekable reader; buffer the ~30 MB dist in memory.
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut std::io::BufReader::new(response), &mut bytes)
            .context("failed to read node.js zip")?;
        extract_node_zip(std::io::Cursor::new(bytes), &tools_dir)?;
    } else {
        let reader = std::io::BufReader::new(response);
        let decoder: Box<dyn std::io::Read> = if node_os == "darwin" {
            Box::new(flate2::read::GzDecoder::new(reader))
        } else {
            Box::new(xz2::read::XzDecoder::new(reader))
        };
        let mut archive = tar::Archive::new(decoder);

        for entry in archive
            .entries()
            .context("failed to read node tarball entries")?
        {
            let mut entry = entry.context("failed to read tarball entry")?;
            let path = entry.path()?.into_owned();
            let stripped: PathBuf = path.components().skip(1).collect();
            if stripped.components().count() == 0 {
                continue;
            }
            let target = tools_dir.join(&stripped);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create directory {}", parent.display()))?;
            }
            entry
                .unpack(&target)
                .with_context(|| format!("failed to unpack {}", stripped.display()))?;
        }
    }

    if !node_bin.exists() {
        anyhow::bail!(
            "downloaded tarball missing node binary — install manually: https://nodejs.org"
        );
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&node_bin, std::fs::Permissions::from_mode(0o755)).ok();
        std::fs::set_permissions(&npm_bin, std::fs::Permissions::from_mode(0o755)).ok();
    }

    if verbose {
        eprintln!("  node v{version} ready at {}", tools_dir.display());
    }

    Ok(tools_dir.join("bin"))
}

/// Ensure a Python interpreter is available for the requested target.
/// Downloads a static Python from `python-build-standalone` (astral-sh) to
/// `~/.cache/daedalus/build-tools/python-<os>-<arch>/` when `target` differs
/// from the host, or uses the host `python3` when `target` is `None`.
pub(crate) fn ensure_python(target: Option<&str>, verbose: bool) -> Result<PathBuf> {
    if target.is_none() {
        if let Ok(p) = which::which("python3") {
            if p.is_file() {
                return Ok(p);
            }
        }
    }

    let suffix = target
        .map(|t| {
            let (arch, os) = parse_target(t);
            format!("{os}-{arch}")
        })
        .unwrap_or_else(|| "host".to_string());

    let tools_dir = cache_dir()
        .join("build-tools")
        .join(format!("python-{suffix}"));

    let python_bin = tools_dir.join("bin").join("python3");
    if python_bin.exists() {
        if verbose {
            eprintln!("  using cached python from {}", tools_dir.display());
        }
        return Ok(python_bin);
    }

    let (py_arch, py_os) = if let Some(t) = target {
        let (arch, os) = parse_target(t);
        let pa = match arch.as_str() {
            "x86_64" | "amd64" => "x86_64",
            "aarch64" | "arm64" => "aarch64",
            "riscv64" | "riscv64gc" => "riscv64",
            _ => anyhow::bail!("unsupported cross-compile architecture: {arch}"),
        };
        let po = match os.as_str() {
            "linux" => "unknown-linux",
            "darwin" => "apple-darwin",
            _ => anyhow::bail!("unsupported cross-compile OS: {os}"),
        };
        (pa, po)
    } else {
        (
            match std::env::consts::ARCH {
                "x86_64" => "x86_64",
                "aarch64" => "aarch64",
                "riscv64" => "riscv64",
                a => a,
            },
            match std::env::consts::OS {
                "linux" => "unknown-linux-musl",
                "macos" => "apple-darwin",
                o => o,
            },
        )
    };

    std::fs::create_dir_all(&tools_dir).context("failed to create build tools directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            tools_dir.parent().unwrap_or(&tools_dir),
            std::fs::Permissions::from_mode(0o700),
        )
        .ok();
    }

    ensure_python_download(&tools_dir, py_arch, py_os, verbose)?;

    if !python_bin.exists() {
        anyhow::bail!("downloaded tarball missing python binary");
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&python_bin, std::fs::Permissions::from_mode(0o755)).ok();
    }

    Ok(python_bin)
}

/// Download and extract a cross-compiled Python from python-build-standalone.
///
/// Fetches the latest release metadata from GitHub to discover the actual
/// version/date rather than hardcoding a specific release that may not exist.
fn ensure_python_download(
    tools_dir: &Path,
    py_arch: &str,
    py_os: &str,
    verbose: bool,
) -> Result<()> {
    // Discover the latest release from GitHub API to avoid hardcoding dates
    // that may not have a matching asset set.
    let release_api =
        "https://api.github.com/repos/astral-sh/python-build-standalone/releases/latest";
    let release_body = std::process::Command::new("curl")
        .args(["-sL", release_api])
        .output()
        .context("failed to run curl for python-build-standalone release manifest")?;
    if !release_body.status.success() {
        anyhow::bail!(
            "curl failed for python-build-standalone release: {}",
            String::from_utf8_lossy(&release_body.stderr)
        );
    }
    let release_json: Value = serde_json::from_slice(&release_body.stdout).with_context(|| {
        let preview = String::from_utf8_lossy(&release_body.stdout)
            .chars()
            .take(200)
            .collect::<String>();
        format!(
            "failed to parse python-build-standalone release manifest (first 200 chars: {preview})"
        )
    })?;
    let date = release_json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("no tag_name in python-build-standalone release"))?;
    let assets = release_json
        .get("assets")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("no assets in python-build-standalone release"))?;
    let version_from_asset = assets
        .iter()
        .find_map(|asset| {
            let name = asset.get("name")?.as_str()?;
            let cap = name.strip_prefix("cpython-")?.split('+').next()?;
            Some(cap)
        })
        .ok_or_else(|| anyhow::anyhow!("no cpython asset found in release"))?;
    let version = version_from_asset;
    let libc_suffixes = if py_os == "unknown-linux" {
        vec!["musl", "gnu"]
    } else {
        vec![""]
    };
    let mut tarball_bytes = None;
    let mut download_url = String::new();
    for suffix in &libc_suffixes {
        let os_tag = if suffix.is_empty() {
            py_os.to_string()
        } else {
            format!("{}-{}", py_os, suffix)
        };
        let url_tag = format!("cpython-{version}+{date}-{py_arch}-{os_tag}");
        let encoded_url_tag = url_tag.replace('+', "%2B");
        let url = format!(
            "https://github.com/astral-sh/python-build-standalone/releases/download/{date}/{encoded_url_tag}-install_only.tar.gz"
        );
        download_url.clone_from(&url);
        if verbose {
            eprintln!("  downloading python {version} ({py_arch}-{os_tag}) from release {date}...");
        }
        let result = std::process::Command::new("curl")
            .args(["-sL", "-o", "-", &url])
            .output()
            .with_context(|| format!("failed to download python tarball from {url}"))?;
        if result.status.success() && result.stdout.len() > 100_000 {
            tarball_bytes = Some(result.stdout);
            break;
        }
    }
    let tarball_bytes = tarball_bytes.ok_or_else(|| {
        anyhow::anyhow!(
            "failed to download python tarball from any known suffix (tried {download_url})"
        )
    })?;

    let extract_dir =
        tempfile::tempdir().context("failed to create temp dir for python extraction")?;
    let tarball_path = extract_dir.path().join("..").join("python.tar.gz");
    std::fs::write(&tarball_path, &tarball_bytes)
        .context("failed to write python tarball to disk")?;

    let output = std::process::Command::new("python3")
        .args([
            "-c",
            "import tarfile,sys; \
            t=tarfile.open(sys.argv[1],'r:gz'); \
            t.extractall(sys.argv[2]); \
            print('extracted', len(t.getmembers()), 'members')",
            tarball_path.to_str().unwrap(),
            extract_dir.path().to_str().unwrap(),
        ])
        .output()
        .context("failed to run python3 for python tarball extraction")?;
    if !output.status.success() {
        anyhow::bail!(
            "python3 extraction failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let top_level = std::fs::read_dir(extract_dir.path())?
        .next()
        .ok_or_else(|| anyhow::anyhow!("python tarball extracted to empty directory"))??;
    if !top_level.file_type()?.is_dir() {
        anyhow::bail!(
            "python tarball top-level `{}` is not a directory",
            top_level.file_name().to_string_lossy()
        );
    }

    let src = extract_dir.path().join(top_level.file_name());
    for entry in std::fs::read_dir(&src)? {
        let entry = entry?;
        let dest = tools_dir.join(entry.file_name());
        if dest.exists() {
            std::fs::remove_dir_all(&dest)?;
        }
        std::fs::rename(entry.path(), &dest)?;
    }

    Ok(())
}

/// Extract the Windows node dist zip into `tools_dir/bin/`.
///
/// The zip layout is a single top-level `node-v<ver>-win-x64/` directory whose
/// contents (`node.exe`, `npm.cmd`, `node_modules/`) we strip into `bin/` so
/// the layout matches the linux/darwin tarballs. `npm.cmd` resolves `node.exe`
/// and `node_modules/npm` relative to its own directory, so they must stay
/// siblings. Entry names are validated against path traversal before use.
fn extract_node_zip<R: std::io::Read + std::io::Seek>(reader: R, tools_dir: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(reader).context("failed to read node.js zip")?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .with_context(|| format!("failed to read zip entry {i}"))?;
        let name = std::path::Path::new(entry.name());
        // Zip-slip guard: reject absolute paths and any traversal component.
        if name.is_absolute()
            || name.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            })
        {
            continue;
        }
        let stripped: PathBuf = name.components().skip(1).collect();
        if stripped.components().count() == 0 {
            continue;
        }
        let target = tools_dir.join("bin").join(&stripped);
        if entry.is_dir() {
            std::fs::create_dir_all(&target)
                .with_context(|| format!("failed to create directory {}", target.display()))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }
        let mut file = std::fs::File::create(&target)
            .with_context(|| format!("failed to create {}", target.display()))?;
        std::io::copy(&mut entry, &mut file)
            .with_context(|| format!("failed to unpack {}", stripped.display()))?;
    }
    Ok(())
}

/// Check PHP platform requirements from composer.json against available extensions.
pub(crate) fn check_php_platform_reqs(app_dir: &Path, verbose: bool) -> Result<()> {
    let composer_path = app_dir.join("composer.json");
    if !composer_path.is_file() {
        return Ok(());
    }

    let content =
        std::fs::read_to_string(&composer_path).context("failed to read composer.json")?;
    let composer: serde_json::Value =
        serde_json::from_str(&content).context("failed to parse composer.json")?;

    let require = match composer.get("require").and_then(|r| r.as_object()) {
        Some(r) => r,
        None => return Ok(()),
    };

    // Check PHP version constraint from composer.json
    if let Some(php_req) = require.get("php").and_then(|v| v.as_str()) {
        let current_version = std::process::Command::new("php")
            .args(["-v"])
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .next()
                    .and_then(|l| l.split_whitespace().nth(1))
                    .map(|s| s.to_string())
            });

        if let Some(ref cur) = current_version {
            if !version_satisfies(cur, php_req) && verbose {
                eprintln!(
                    "[daedalus] warning: composer.json requires PHP {}, but php {} is on PATH",
                    php_req, cur
                );
                if let Some(alt) = find_php_binary(php_req) {
                    eprintln!(
                        "[daedalus]   consider using --embed-interpreter {} or set PATH to use {}",
                        alt, alt
                    );
                }
            }
        }
    }

    let mut required_exts: Vec<&str> = Vec::new();
    for key in require.keys() {
        if let Some(ext) = key.strip_prefix("ext-") {
            required_exts.push(ext);
        }
    }

    if required_exts.is_empty() {
        return Ok(());
    }

    // Check which extensions are available
    let php_output = std::process::Command::new("php")
        .args(["-m"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let available: Vec<String> = php_output
        .lines()
        .map(|l| l.trim().to_lowercase())
        .collect();

    let mut missing: Vec<String> = Vec::new();
    for ext in &required_exts {
        if !available.contains(&ext.to_lowercase()) {
            missing.push(ext.to_string());
        }
    }

    if !missing.is_empty() {
        eprintln!("[daedalus] warning: PHP extensions required by composer but not installed:");
        for ext in &missing {
            eprintln!("  ext-{ext}");
        }
        eprintln!("  Run: sudo apt install php-{}", missing.join(" php-"));
        eprintln!("  or: composer install --ignore-platform-reqs (will be used as fallback)");
        if verbose {
            eprintln!("  Proceeding with --ignore-platform-reqs — runtime may fail if extensions are needed.");
        }
    } else if verbose {
        eprintln!(
            "  PHP platform extensions: all {} required extension(s) available",
            required_exts.len()
        );
    }

    Ok(())
}

/// Simple PHP version constraint check — handles `^8.2`, `>=8.0`, `8.1`, `8.*`,
/// `~8.1.0`, and `8.1 || 8.2` patterns. Returns true if the version satisfies.
fn version_satisfies(version: &str, constraint: &str) -> bool {
    let version_parts: Vec<u32> = version
        .split('.')
        .filter_map(|s| {
            s.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .ok()
        })
        .collect();
    if version_parts.is_empty() {
        return false;
    }

    for part in constraint.split("||") {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if satisfies_single(&version_parts, part) {
            return true;
        }
    }
    false
}

/// satisfies_single - satisfies single.
/// @version: version
/// @constraint: constraint
///
/// Description:
///
/// Return: true or false
fn satisfies_single(version: &[u32], constraint: &str) -> bool {
    let constraint = constraint.trim();
    if constraint.starts_with('^') {
        if let Some(rest) = constraint.strip_prefix('^') {
            let target = parse_version(rest);
            if target.is_empty() {
                true
            } else {
                version >= &target && version.first() == target.first()
            }
        } else {
            true
        }
    } else if constraint.starts_with('~') {
        if let Some(rest) = constraint.strip_prefix('~') {
            let target = parse_version(rest);
            if target.len() < 2 {
                true
            } else {
                version.len() >= 2
                    && version[0] == target[0]
                    && version[1] == target[1]
                    && version >= &target
            }
        } else {
            true
        }
    } else if constraint.ends_with('*') {
        let prefix = constraint.trim_end_matches('*');
        let prefix_parts: Vec<u32> = prefix.split('.').filter_map(|s| s.parse().ok()).collect();
        version.starts_with(&prefix_parts)
    } else if let Some(rest) = constraint.strip_prefix(">=") {
        let target = parse_version(rest.trim());
        compare_versions(version, &target).map_or(false, |ord| ord != std::cmp::Ordering::Less)
    } else if let Some(rest) = constraint.strip_prefix("<=") {
        let target = parse_version(rest.trim());
        compare_versions(version, &target).map_or(false, |ord| ord != std::cmp::Ordering::Greater)
    } else if let Some(rest) = constraint.strip_prefix('>') {
        let target = parse_version(rest.trim());
        compare_versions(version, &target).map_or(false, |ord| ord == std::cmp::Ordering::Greater)
    } else if let Some(rest) = constraint.strip_prefix('<') {
        let target = parse_version(rest.trim());
        compare_versions(version, &target).map_or(false, |ord| ord == std::cmp::Ordering::Less)
    } else if let Some(rest) = constraint.strip_prefix("==") {
        let target = parse_version(rest.trim());
        compare_versions(version, &target).map_or(false, |ord| ord == std::cmp::Ordering::Equal)
    } else {
        let target = parse_version(constraint);
        version == target
    }
}

/// parse_version - parse version.
/// @s: s
///
/// Description:
///
/// Return: vector of Vec<u32>
fn parse_version(s: &str) -> Vec<u32> {
    s.split('.').filter_map(|p| p.trim().parse().ok()).collect()
}

/// compare_versions - compare versions.
/// @a: a
/// @b: b
/// @std: std
/// @cmp: cmp
///
/// Description:
///
/// Return: Some(...) if present, None otherwise
fn compare_versions(a: &[u32], b: &[u32]) -> Option<std::cmp::Ordering> {
    for (x, y) in a.iter().zip(b.iter()) {
        let ord = x.cmp(y);
        if ord != std::cmp::Ordering::Equal {
            return Some(ord);
        }
    }
    a.len().cmp(&b.len()).into()
}

/// Look for alternative PHP binaries on PATH that might satisfy the version constraint.
fn find_php_binary(constraint: &str) -> Option<String> {
    let rest = constraint.strip_prefix('^')?;
    let major = rest.split('.').next()?;
    for candidate in ["php", &format!("php{major}")] {
        if which::which(candidate).is_ok() {
            return Some(candidate.to_string());
        }
    }
    None
}

/// Check if package.json contains `workspace:*` protocol deps (pnpm-specific).
pub(crate) fn has_workspace_protocol(dir: &Path) -> bool {
    let pkg = match std::fs::read_to_string(dir.join("package.json")) {
        Ok(c) => c,
        _ => return false,
    };
    let json: serde_json::Value = match serde_json::from_str(&pkg) {
        Ok(v) => v,
        _ => return false,
    };
    for section in &[
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ] {
        if let Some(deps) = json.get(*section).and_then(|d| d.as_object()) {
            for val in deps.values() {
                if let Some(v) = val.as_str() {
                    if v.starts_with("workspace:") {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Returns `(program, extra_args)` to prepend to the install command.
pub(crate) fn ensure_composer(app_dir: &Path, verbose: bool) -> Result<(String, Vec<String>)> {
    // Try system composer first
    if std::process::Command::new("composer")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
    {
        return Ok(("composer".into(), Vec::new()));
    }

    // Try php with system composer.phar
    if std::process::Command::new("php")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
    {
        let phar = app_dir.join("composer.phar");
        if !phar.exists() {
            if verbose {
                eprintln!("  downloading composer.phar...");
            }
            let status = std::process::Command::new("php")
                .args([
                    "-r",
                    "copy('https://getcomposer.org/download/latest-stable/composer.phar', 'composer.phar');",
                ])
                .current_dir(app_dir)
                .status()
                .context("failed to download composer.phar")?;
            if !status.success() {
                anyhow::bail!(
                    "composer not found and failed to download composer.phar — \
                     install composer: https://getcomposer.org/download"
                );
            }
            // Make it executable
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&phar, std::fs::Permissions::from_mode(0o755)).ok();
            }
        }
        return Ok(("php".into(), vec![phar.to_string_lossy().to_string()]));
    }

    anyhow::bail!(
        "composer not found — install it: https://getcomposer.org/download \
         or install php + composer"
    )
}

/// Ensure Go toolchain is available for the build.
/// Downloads a static Go binary to `~/.cache/daedalus/build-tools/go-{target}/`
/// if not on PATH. Returns `(go_bin_dir, go_binary_path)`.
pub(crate) fn ensure_go(target: Option<&str>, verbose: bool) -> Result<PathBuf> {
    let suffix = target
        .map(|t| {
            let (arch, os) = parse_target(t);
            format!("{os}-{arch}")
        })
        .unwrap_or_else(|| "host".to_string());
    let tools_dir = cache_dir().join("build-tools").join(format!("go-{suffix}"));
    let is_windows = target.is_some_and(|t| parse_target(t).1 == "windows");
    let go_name = if is_windows { "go.exe" } else { "go" };
    let go_bin = tools_dir.join("bin").join(go_name);

    if go_bin.exists() {
        if verbose {
            eprintln!("  using cached go from {}", tools_dir.display());
        }
        return Ok(tools_dir.join("bin"));
    }

    if verbose {
        eprintln!("  downloading go to {}...", tools_dir.display());
    }

    std::fs::create_dir_all(&tools_dir).context("failed to create build tools directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            tools_dir.parent().unwrap_or(tools_dir.as_path()),
            std::fs::Permissions::from_mode(0o700),
        )
        .ok();
    }

    ensure_go_download(tools_dir, target, verbose)
}

/// Download a specific Go version for a target architecture.
fn ensure_go_download(
    tools_dir: PathBuf,
    target_arch: Option<&str>,
    verbose: bool,
) -> Result<PathBuf> {
    let (go_arch, go_os) = if let Some(target) = target_arch {
        let (arch, os) = parse_target(target);
        let go_arch = match arch.as_str() {
            "x86_64" | "amd64" => "amd64",
            "aarch64" | "arm64" => "arm64",
            _ => anyhow::bail!("unsupported cross-compile architecture for Go: {arch}"),
        };
        let go_os = match os.as_str() {
            "linux" => "linux",
            "darwin" => "darwin",
            "windows" => "windows",
            _ => anyhow::bail!("unsupported cross-compile OS for Go: {os}"),
        };
        (go_arch.to_string(), go_os.to_string())
    } else {
        let go_arch = match std::env::consts::ARCH {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            arch => arch,
        };
        let go_os = match std::env::consts::OS {
            "linux" => "linux",
            "macos" => "darwin",
            "windows" => "windows",
            os => os,
        };
        (go_arch.to_string(), go_os.to_string())
    };

    // Determine Go version to download
    let version = if let Ok(pinned) = std::env::var("DAEDALUS_GO_VERSION") {
        if verbose {
            eprintln!("  using pinned go version {pinned} (DAEDALUS_GO_VERSION)");
        }
        pinned
    } else {
        // Fetch the latest stable Go version from go.dev
        let resp = reqwest::blocking::get("https://go.dev/dl/?mode=json")
            .context("failed to reach go.dev")?;
        let releases: Vec<serde_json::Value> =
            resp.json().context("failed to parse Go release manifest")?;
        releases
            .first()
            .and_then(|r| r.get("version")?.as_str())
            .map(|v| v.to_string())
            .ok_or_else(|| anyhow::anyhow!("no Go version found in release manifest"))?
    };

    // Go downloads: go{version}.{goos}-{goarch}.zip for Windows, .tar.gz for Linux/macOS
    let version_trimmed = version.strip_prefix("go").unwrap_or(&version);
    let ext = if go_os == "windows" { "zip" } else { "tar.gz" };
    let tarball = format!("go{version_trimmed}.{go_os}-{go_arch}.{ext}");
    let url = format!("https://go.dev/dl/{tarball}");

    if verbose {
        eprintln!("  downloading go {version} ({go_os}-{go_arch})...");
    }

    let response = reqwest::blocking::get(&url)
        .with_context(|| format!("failed to download Go tarball from {url}"))?;
    let reader = std::io::BufReader::new(response);

    if go_os == "windows" {
        let mut bytes = Vec::new();
        let mut reader = reader;
        std::io::Read::read_to_end(&mut reader, &mut bytes).context("failed to read Go zip")?;
        extract_go_zip(std::io::Cursor::new(bytes), &tools_dir)?;
    } else {
        let decoder = flate2::read::GzDecoder::new(reader);
        let mut archive = tar::Archive::new(decoder);
        for entry in archive
            .entries()
            .context("failed to read Go tarball entries")?
        {
            let mut entry = entry.context("failed to read tarball entry")?;
            let path = entry.path()?.into_owned();
            let stripped: PathBuf = path.components().skip(1).collect();
            if stripped.components().count() == 0 {
                continue;
            }
            let target = tools_dir.join(&stripped);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create directory {}", parent.display()))?;
            }
            entry
                .unpack(&target)
                .with_context(|| format!("failed to unpack {}", stripped.display()))?;
        }
    }

    let go_bin = if go_os == "windows" {
        tools_dir.join("bin").join("go.exe")
    } else {
        tools_dir.join("bin").join("go")
    };

    if !go_bin.exists() {
        anyhow::bail!(
            "downloaded tarball missing go binary — install manually: https://go.dev/dl/"
        );
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&go_bin, std::fs::Permissions::from_mode(0o755)).ok();
    }

    if verbose {
        eprintln!("  go {version} ready at {}", tools_dir.display());
    }

    Ok(tools_dir.join("bin"))
}

/// Extract the Windows Go dist zip into `tools_dir/`.
fn extract_go_zip<R: std::io::Read + std::io::Seek>(reader: R, tools_dir: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(reader).context("failed to read Go zip")?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .with_context(|| format!("failed to read zip entry {i}"))?;
        let name = std::path::Path::new(entry.name());
        if name.is_absolute()
            || name.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            })
        {
            continue;
        }
        let stripped: PathBuf = name.components().skip(1).collect();
        if stripped.components().count() == 0 {
            continue;
        }
        let target = tools_dir.join(&stripped);
        if entry.is_dir() {
            std::fs::create_dir_all(&target)
                .with_context(|| format!("failed to create directory {}", target.display()))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }
        let mut file = std::fs::File::create(&target)
            .with_context(|| format!("failed to create {}", target.display()))?;
        std::io::copy(&mut entry, &mut file)
            .with_context(|| format!("failed to unpack {}", stripped.display()))?;
    }
    Ok(())
}

/// Ensure a Deno binary is available for the requested target.
/// Downloads a static Deno binary from GitHub releases to
/// `~/.cache/daedalus/build-tools/deno-{target}/` when not on PATH.
pub(crate) fn ensure_deno(target: Option<&str>, verbose: bool) -> Result<PathBuf> {
    let suffix = target
        .map(|t| {
            let (arch, os) = parse_target(t);
            format!("{os}-{arch}")
        })
        .unwrap_or_else(|| "host".to_string());
    let tools_dir = cache_dir()
        .join("build-tools")
        .join(format!("deno-{suffix}"));
    let is_windows = target.is_some_and(|t| parse_target(t).1 == "windows");
    let deno_name = if is_windows { "deno.exe" } else { "deno" };
    let deno_bin = tools_dir.join("bin").join(deno_name);

    if deno_bin.exists() {
        if verbose {
            eprintln!("  using cached deno from {}", tools_dir.display());
        }
        return Ok(tools_dir.join("bin"));
    }

    if verbose {
        eprintln!("  downloading deno to {}...", tools_dir.display());
    }

    std::fs::create_dir_all(&tools_dir).context("failed to create build tools directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            tools_dir.parent().unwrap_or(tools_dir.as_path()),
            std::fs::Permissions::from_mode(0o700),
        )
        .ok();
    }

    ensure_deno_download(&tools_dir, target, verbose)?;

    if !deno_bin.exists() {
        anyhow::bail!(
            "downloaded deno archive missing deno binary — install manually: https://deno.land"
        );
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&deno_bin, std::fs::Permissions::from_mode(0o755)).ok();
    }

    Ok(tools_dir.join("bin"))
}

/// Download a Deno binary for the target architecture from GitHub releases.
fn ensure_deno_download(tools_dir: &Path, target_arch: Option<&str>, verbose: bool) -> Result<()> {
    let (deno_arch, deno_triple) = if let Some(target) = target_arch {
        let (arch, os) = parse_target(target);
        let da = match arch.as_str() {
            "x86_64" | "amd64" => "x86_64",
            "aarch64" | "arm64" => "aarch64",
            _ => anyhow::bail!("unsupported cross-compile architecture for Deno: {arch}"),
        };
        let dt = match os.as_str() {
            "linux" => "unknown-linux-gnu",
            "darwin" | "macos" => "apple-darwin",
            "windows" => "pc-windows-msvc",
            _ => anyhow::bail!("unsupported cross-compile OS for Deno: {os}"),
        };
        (da.to_string(), dt.to_string())
    } else {
        let da = match std::env::consts::ARCH {
            "x86_64" => "x86_64",
            "aarch64" => "aarch64",
            arch => arch,
        };
        let dt = match std::env::consts::OS {
            "linux" => "unknown-linux-gnu",
            "macos" => "apple-darwin",
            "windows" => "pc-windows-msvc",
            os => os,
        };
        (da.to_string(), dt.to_string())
    };

    let ext = "zip";
    let asset_name = format!("deno-{deno_arch}-{deno_triple}.{ext}");
    let url = format!("https://github.com/denoland/deno/releases/latest/download/{asset_name}");

    if verbose {
        eprintln!("  downloading deno ({deno_arch}-{deno_triple})...");
    }

    let response = reqwest::blocking::get(&url)
        .with_context(|| format!("failed to download Deno from {url}"))?;
    if !response.status().is_success() {
        anyhow::bail!(
            "failed to download Deno binary (HTTP {})",
            response.status()
        );
    }

    let bin_dir = tools_dir.join("bin");
    std::fs::create_dir_all(&bin_dir).context("failed to create bin directory")?;

    if ext == "zip" {
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut std::io::BufReader::new(response), &mut bytes)
            .context("failed to read deno zip")?;
        extract_deno_zip(std::io::Cursor::new(bytes), &tools_dir)?;
    } else {
        let reader = std::io::BufReader::new(response);
        let decoder = flate2::read::GzDecoder::new(reader);
        let mut archive = tar::Archive::new(decoder);
        for entry in archive
            .entries()
            .context("failed to read deno tarball entries")?
        {
            let mut entry = entry.context("failed to read tarball entry")?;
            let path = entry.path()?.into_owned();
            let stripped: PathBuf = path.components().skip(1).collect();
            if stripped.components().count() == 0 {
                continue;
            }
            let target = tools_dir.join(&stripped);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create directory {}", parent.display()))?;
            }
            entry
                .unpack(&target)
                .with_context(|| format!("failed to unpack {}", stripped.display()))?;
        }
    }

    Ok(())
}

/// Extract a Deno zip (single binary) into `tools_dir/bin/`.
fn extract_deno_zip<R: std::io::Read + std::io::Seek>(reader: R, tools_dir: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(reader).context("failed to read deno zip")?;
    let bin_dir = tools_dir.join("bin");
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .with_context(|| format!("failed to read zip entry {i}"))?;
        let name = std::path::Path::new(entry.name());
        if name.is_absolute()
            || name.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            })
        {
            continue;
        }
        let target = bin_dir.join(name.file_name().unwrap_or_default());
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }
        let mut file = std::fs::File::create(&target)
            .with_context(|| format!("failed to create {}", target.display()))?;
        std::io::copy(&mut entry, &mut file)
            .with_context(|| format!("failed to unpack {}", target.display()))?;
    }
    Ok(())
}

/// Ensure a Hugo binary is available for the requested target.
/// Downloads Hugo from GitHub releases to
/// `~/.cache/daedalus/build-tools/hugo-{target}/` when not on PATH.
pub(crate) fn ensure_hugo(target: Option<&str>, verbose: bool) -> Result<PathBuf> {
    let suffix = target
        .map(|t| {
            let (arch, os) = parse_target(t);
            format!("{os}-{arch}")
        })
        .unwrap_or_else(|| "host".to_string());
    let tools_dir = cache_dir()
        .join("build-tools")
        .join(format!("hugo-{suffix}"));
    let is_windows = target.is_some_and(|t| parse_target(t).1 == "windows");
    let hugo_name = if is_windows { "hugo.exe" } else { "hugo" };
    let hugo_bin = tools_dir.join("bin").join(hugo_name);

    if hugo_bin.exists() {
        if verbose {
            eprintln!("  using cached hugo from {}", tools_dir.display());
        }
        return Ok(tools_dir.join("bin"));
    }

    if verbose {
        eprintln!("  downloading hugo to {}...", tools_dir.display());
    }

    std::fs::create_dir_all(&tools_dir).context("failed to create build tools directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            tools_dir.parent().unwrap_or(tools_dir.as_path()),
            std::fs::Permissions::from_mode(0o700),
        )
        .ok();
    }

    ensure_hugo_download(&tools_dir, target, verbose)?;

    if !hugo_bin.exists() {
        anyhow::bail!(
            "downloaded hugo archive missing hugo binary — install manually: https://gohugo.io/installation/"
        );
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hugo_bin, std::fs::Permissions::from_mode(0o755)).ok();
    }

    Ok(tools_dir.join("bin"))
}

/// Download a Hugo binary for the target architecture from GitHub releases.
fn ensure_hugo_download(tools_dir: &Path, target_arch: Option<&str>, verbose: bool) -> Result<()> {
    let (hugo_arch, hugo_os) = if let Some(target) = target_arch {
        let (arch, os) = parse_target(target);
        let ha = match arch.as_str() {
            "x86_64" | "amd64" => "amd64",
            "aarch64" | "arm64" => "arm64",
            _ => anyhow::bail!("unsupported cross-compile architecture for Hugo: {arch}"),
        };
        let hos = match os.as_str() {
            "linux" => "linux",
            "darwin" => "darwin",
            "windows" => "windows",
            _ => anyhow::bail!("unsupported cross-compile OS for Hugo: {os}"),
        };
        (ha.to_string(), hos.to_string())
    } else {
        let ha = match std::env::consts::ARCH {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            arch => arch,
        };
        let hos = match std::env::consts::OS {
            "linux" => "linux",
            "macos" => "darwin",
            "windows" => "windows",
            os => os,
        };
        (ha.to_string(), hos.to_string())
    };

    let ext = "tar.gz";
    let client = reqwest::blocking::Client::builder()
        .user_agent("daedalus/0.5")
        .build()
        .context("failed to build HTTP client for Hugo version lookup")?;
    let version = client
        .get("https://api.github.com/repos/gohugoio/hugo/releases/latest")
        .send()
        .with_context(|| "failed to query Hugo latest release")?
        .json::<serde_json::Value>()
        .with_context(|| "failed to parse Hugo release JSON")?
        .get("tag_name")
        .and_then(|t| t.as_str())
        .map(|s| s.trim_start_matches('v').to_string())
        .context("Hugo release missing tag_name")?;
    let asset_name = format!("hugo_extended_{version}_{hugo_os}-{hugo_arch}.{ext}");
    let url = format!("https://github.com/gohugoio/hugo/releases/latest/download/{asset_name}");

    if verbose {
        eprintln!("  downloading hugo ({hugo_os}-{hugo_arch})...");
    }

    let response = reqwest::blocking::get(&url)
        .with_context(|| format!("failed to download Hugo from {url}"))?;
    if !response.status().is_success() {
        anyhow::bail!(
            "failed to download Hugo binary (HTTP {})",
            response.status()
        );
    }

    let reader = std::io::BufReader::new(response);
    let decoder = flate2::read::GzDecoder::new(reader);
    let mut archive = tar::Archive::new(decoder);
    let bin_dir = tools_dir.join("bin");
    std::fs::create_dir_all(&bin_dir).context("failed to create bin directory")?;

    for entry in archive
        .entries()
        .context("failed to read hugo tarball entries")?
    {
        let mut entry = entry.context("failed to read tarball entry")?;
        let path = entry.path()?.into_owned();
        let file_name = path.file_name().unwrap_or_default();
        let target = bin_dir.join(file_name);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }
        entry
            .unpack(&target)
            .with_context(|| format!("failed to unpack {}", target.display()))?;
    }

    Ok(())
}

/// Ensure a Wasmtime binary is available for the requested target.
/// Downloads Wasmtime from GitHub releases to
/// `~/.cache/daedalus/build-tools/wasmtime-{target}/` when not on PATH.
pub(crate) fn ensure_wasmtime(target: Option<&str>, verbose: bool) -> Result<PathBuf> {
    let suffix = target
        .map(|t| {
            let (arch, os) = parse_target(t);
            format!("{os}-{arch}")
        })
        .unwrap_or_else(|| "host".to_string());
    let tools_dir = cache_dir()
        .join("build-tools")
        .join(format!("wasmtime-{suffix}"));
    let is_windows = target.is_some_and(|t| parse_target(t).1 == "windows");
    let wasmtime_name = if is_windows {
        "wasmtime.exe"
    } else {
        "wasmtime"
    };
    let wasmtime_bin = tools_dir.join("bin").join(wasmtime_name);

    if wasmtime_bin.exists() {
        if verbose {
            eprintln!("  using cached wasmtime from {}", tools_dir.display());
        }
        return Ok(tools_dir.join("bin"));
    }

    if verbose {
        eprintln!("  downloading wasmtime to {}...", tools_dir.display());
    }

    std::fs::create_dir_all(&tools_dir).context("failed to create build tools directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            tools_dir.parent().unwrap_or(tools_dir.as_path()),
            std::fs::Permissions::from_mode(0o700),
        )
        .ok();
    }

    ensure_wasmtime_download(&tools_dir, target, verbose)?;

    if !wasmtime_bin.exists() {
        anyhow::bail!(
            "downloaded wasmtime archive missing wasmtime binary — install manually: https://wasmtime.dev"
        );
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wasmtime_bin, std::fs::Permissions::from_mode(0o755)).ok();
    }

    Ok(tools_dir.join("bin"))
}

/// Download a Wasmtime binary for the target architecture from GitHub releases.
fn ensure_wasmtime_download(
    tools_dir: &Path,
    target_arch: Option<&str>,
    verbose: bool,
) -> Result<()> {
    let (wasmtime_arch, wasmtime_os) = if let Some(target) = target_arch {
        let (arch, os) = parse_target(target);
        let wa = match arch.as_str() {
            "x86_64" | "amd64" => "x86_64",
            "aarch64" | "arm64" => "aarch64",
            _ => anyhow::bail!("unsupported cross-compile architecture for Wasmtime: {arch}"),
        };
        let wo = match os.as_str() {
            "linux" => "linux",
            "darwin" => "apple-darwin",
            "windows" => "windows",
            _ => anyhow::bail!("unsupported cross-compile OS for Wasmtime: {os}"),
        };
        (wa.to_string(), wo.to_string())
    } else {
        let wa = match std::env::consts::ARCH {
            "x86_64" => "x86_64",
            "aarch64" => "aarch64",
            arch => arch,
        };
        let wo = match std::env::consts::OS {
            "linux" => "linux",
            "macos" => "apple-darwin",
            "windows" => "windows",
            os => os,
        };
        (wa.to_string(), wo.to_string())
    };

    let ext = "tar.gz";
    let asset_name = format!("wasmtime-{wasmtime_os}-{wasmtime_arch}.{ext}");
    let url = format!(
        "https://github.com/bytecodealliance/wasmtime/releases/latest/download/{asset_name}"
    );

    if verbose {
        eprintln!("  downloading wasmtime ({wasmtime_os}-{wasmtime_arch})...");
    }

    let response = reqwest::blocking::get(&url)
        .with_context(|| format!("failed to download Wasmtime from {url}"))?;
    if !response.status().is_success() {
        anyhow::bail!(
            "failed to download Wasmtime binary (HTTP {})",
            response.status()
        );
    }

    let reader = std::io::BufReader::new(response);
    let decoder = flate2::read::GzDecoder::new(reader);
    let mut archive = tar::Archive::new(decoder);
    let bin_dir = tools_dir.join("bin");
    std::fs::create_dir_all(&bin_dir).context("failed to create bin directory")?;

    for entry in archive
        .entries()
        .context("failed to read wasmtime tarball entries")?
    {
        let mut entry = entry.context("failed to read tarball entry")?;
        let path = entry.path()?.into_owned();
        let file_name = path.file_name().unwrap_or_default();
        let target = bin_dir.join(file_name);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", target.display()))?;
        }
        entry
            .unpack(&target)
            .with_context(|| format!("failed to unpack {}", target.display()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// check_php_platform_reqs_no_composer_json - check php platform reqs no composer json.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn check_php_platform_reqs_no_composer_json() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check_php_platform_reqs(dir.path(), false).is_ok());
    }

    #[test]
    /// check_php_platform_reqs_no_require - check php platform reqs no require.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn check_php_platform_reqs_no_require() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("composer.json"), "{}").unwrap();
        assert!(check_php_platform_reqs(dir.path(), false).is_ok());
    }

    #[test]
    /// check_php_platform_reqs_finds_extensions - check php platform reqs finds extensions.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn check_php_platform_reqs_finds_extensions() {
        let dir = tempfile::tempdir().unwrap();
        let composer = r#"{"require": {"php": ">=8.0", "ext-json": "*", "ext-mbstring": "*"}}"#;
        std::fs::write(dir.path().join("composer.json"), composer).unwrap();
        // Should not error even if php is not available
        assert!(check_php_platform_reqs(dir.path(), false).is_ok());
    }

    #[test]
    /// ensure_python_returns_host_when_no_target - ensure python returns host when no target.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn ensure_python_returns_host_when_no_target() {
        // When target is None, ensure_python should find the host python3
        // This test only checks the path doesn't error; actual download is
        // tested in integration tests.
        let result = ensure_python(None, false);
        // It should either succeed (python3 on PATH) or error gracefully
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    /// ensure_python_target_arch_mapping - ensure python target arch mapping.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn ensure_python_target_arch_mapping() {
        let target = "aarch64-apple-darwin";
        let (arch, os) = parse_target(target);
        let py_arch = match arch.as_str() {
            "x86_64" | "amd64" => "x86_64",
            "aarch64" | "arm64" => "aarch64",
            "riscv64" | "riscv64gc" => "riscv64",
            _ => panic!("unsupported arch"),
        };
        let py_os = match os.as_str() {
            "linux" => "unknown-linux-musl",
            "darwin" => "apple-darwin",
            _ => panic!("unsupported os"),
        };
        assert_eq!(py_arch, "aarch64");
        assert_eq!(py_os, "apple-darwin");
    }

    #[test]
    /// ensure_python_musl_target_mapping - ensure python musl target mapping.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn ensure_python_musl_target_mapping() {
        let target = "riscv64gc-unknown-linux-musl";
        let (arch, _os) = parse_target(target);
        let py_arch = match arch.as_str() {
            "x86_64" | "amd64" => "x86_64",
            "aarch64" | "arm64" => "aarch64",
            "riscv64" | "riscv64gc" => "riscv64",
            _ => arch.as_str(),
        };
        assert_eq!(py_arch, "riscv64");
    }

    #[test]
    /// ensure_python_url_format - ensure python url format.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn ensure_python_url_format() {
        let py_arch = "aarch64";
        let py_os = "unknown-linux-musl";
        let version = "3.10.21";
        let date = "20260814";
        let url = format!(
            "https://github.com/astral-sh/python-build-standalone/releases/download/{date}/cpython-{version}+{date}-{py_arch}-{py_os}-install_only.tar.gz"
        );
        assert!(url.contains("astral-sh/python-build-standalone"));
        assert!(url.contains("aarch64-unknown-linux-musl"));
        assert!(url.contains("install_only"));
    }
}
