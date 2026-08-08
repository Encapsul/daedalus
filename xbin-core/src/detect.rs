//! Runtime detection — identifies which runtime an app directory uses.
//!
//! Detection order matches the Python registry:
//! Python > Deno > Node > Java > Ruby > .NET > Go > PHP > Perl > Binary

use std::io::Read;
use std::path::Path;

/// Detected runtime type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    Python,
    Deno,
    Node,
    Java,
    Ruby,
    Dotnet,
    Go,
    Php,
    Perl,
    Hugo,
    Binary,
}

impl Runtime {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::Deno => "deno",
            Self::Node => "node",
            Self::Java => "java",
            Self::Ruby => "ruby",
            Self::Dotnet => "dotnet",
            Self::Go => "go",
            Self::Php => "php",
            Self::Perl => "perl",
            Self::Hugo => "hugo",
            Self::Binary => "binary",
        }
    }
}

/// Detect the runtime for an app directory by checking marker files.
/// Returns the first match in priority order, preferring runtimes with entry files.
pub fn detect_runtime(app_dir: &Path) -> Option<Runtime> {
    let detected = detect_runtime_candidates(app_dir);

    // Check for entry files and prefer runtimes that have them
    let php_has_entry = detect_php(app_dir)
        && (app_dir.join("index.php").is_file()
            || app_dir.join("public/index.php").is_file()
            || app_dir.join("artisan").is_file()
            || app_dir.join("entry.php").is_file());
    let node_has_entry = detect_node(app_dir) && find_node_entry(app_dir).is_some();
    let python_has_entry = detect_python(app_dir)
        && find_first_file(app_dir, &["app.py", "main.py", "__main__.py", "server.py"]).is_some();

    // Priority: prefer runtime with entry file, then fallback to detection order
    if php_has_entry && detected.iter().any(|(r, _)| *r == Runtime::Php) {
        return Some(Runtime::Php);
    }
    if node_has_entry && detected.iter().any(|(r, _)| *r == Runtime::Node) {
        return Some(Runtime::Node);
    }
    if python_has_entry && detected.iter().any(|(r, _)| *r == Runtime::Python) {
        return Some(Runtime::Python);
    }

    detected.into_iter().next().map(|(r, _)| r)
}

/// Detect all runtime candidates, returning (runtime, `has_marker`) pairs.
fn detect_runtime_candidates(dir: &Path) -> Vec<(Runtime, bool)> {
    let mut candidates = Vec::new();

    if detect_python(dir) {
        candidates.push((Runtime::Python, true));
    }
    if detect_deno(dir) {
        candidates.push((Runtime::Deno, true));
    }
    if detect_node(dir) {
        candidates.push((Runtime::Node, true));
    }
    if detect_java(dir) {
        candidates.push((Runtime::Java, true));
    }
    if detect_ruby(dir) {
        candidates.push((Runtime::Ruby, true));
    }
    if detect_dotnet(dir) {
        candidates.push((Runtime::Dotnet, true));
    }
    if detect_go(dir) {
        candidates.push((Runtime::Go, true));
    }
    if detect_php(dir) {
        candidates.push((Runtime::Php, true));
    }
    if detect_perl(dir) {
        candidates.push((Runtime::Perl, true));
    }
    if detect_hugo(dir) {
        candidates.push((Runtime::Hugo, true));
    }
    if detect_binary(dir) {
        candidates.push((Runtime::Binary, true));
    }

    candidates
}

fn detect_python(dir: &Path) -> bool {
    ["app.py", "main.py", "__main__.py", "server.py"]
        .iter()
        .any(|f| dir.join(f).is_file())
        || dir.join("pyproject.toml").is_file()
        || dir.join("setup.py").is_file()
        || dir.join("requirements.txt").is_file()
}

fn detect_deno(dir: &Path) -> bool {
    dir.join("deno.json").is_file() || dir.join("deno.jsonc").is_file()
}

fn detect_node(dir: &Path) -> bool {
    dir.join("package.json").is_file()
}

fn detect_java(dir: &Path) -> bool {
    dir.join("pom.xml").is_file()
        || dir.join("build.gradle").is_file()
        || dir.join("build.gradle.kts").is_file()
}

fn detect_ruby(dir: &Path) -> bool {
    dir.join("Gemfile").is_file()
}

fn detect_dotnet(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .any(|e| e.path().extension().is_some_and(|ext| ext == "csproj"))
        })
        .unwrap_or(false)
}

fn detect_go(dir: &Path) -> bool {
    dir.join("go.mod").is_file()
}

fn detect_php(dir: &Path) -> bool {
    // 1. Composer projects (most common)
    if dir.join("composer.json").is_file() {
        return true;
    }
    // 2. PHP files exist in the directory or one level deep
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if path.extension().and_then(|e| e.to_str()) == Some("php") {
                    return true;
                }
            } else if path.is_dir() {
                if let Ok(sub_entries) = std::fs::read_dir(&path) {
                    for sub_entry in sub_entries.flatten() {
                        let sub_path = sub_entry.path();
                        if sub_path.is_file()
                            && sub_path.extension().and_then(|e| e.to_str()) == Some("php")
                        {
                            return true;
                        }
                    }
                }
            }
        }
    }
    // 3. PHP config files
    if dir.join("php.ini").exists() {
        return true;
    }
    false
}

fn detect_perl(dir: &Path) -> bool {
    dir.join("Makefile.PL").is_file() || dir.join("cpanfile").is_file()
}

fn detect_hugo(dir: &Path) -> bool {
    dir.join("config.toml").is_file()
        || dir.join("hugo.toml").is_file()
        || dir.join("config.yaml").is_file()
}

fn detect_binary(dir: &Path) -> bool {
    let mut native_count = 0;
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return false,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if is_native_binary(&path) {
            native_count += 1;
            if native_count > 1 {
                return false;
            }
        }
    }
    native_count == 1
}

/// True if `path` is an ELF or PE (`.exe`) executable by magic bytes.
fn is_native_binary(path: &Path) -> bool {
    let mut magic = [0u8; 4];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .map(|()| &magic[..] == b"\x7fELF" || (magic[0] == b'M' && magic[1] == b'Z'))
        .unwrap_or(false)
}

/// Resolve the entrypoint argv for a detected runtime.
///
/// Returns `None` if the entry file cannot be determined.
/// Interpreter names are bare (e.g. `python3`, `node`) — the stub uses
/// `execvp` which resolves them via PATH. App paths use `/app/` prefix.
pub fn resolve_entrypoint(app_dir: &Path, runtime: Runtime) -> Option<Vec<String>> {
    match runtime {
        Runtime::Python => {
            let entry =
                find_first_file(app_dir, &["app.py", "main.py", "__main__.py", "server.py"])?;
            Some(vec!["python3".into(), format!("/app/{}", entry)])
        }
        Runtime::Node => {
            let entry = find_node_entry(app_dir)?;
            Some(vec!["node".into(), format!("/app/{}", entry)])
        }
        Runtime::Deno => {
            let entry = find_first_file(app_dir, &["main.ts", "main.js", "index.ts", "index.js"])?;
            Some(vec![
                "deno".into(),
                "run".into(),
                "--allow-all".into(),
                format!("/app/{}", entry),
            ])
        }
        Runtime::Go | Runtime::Binary => {
            let bin = find_native_binary(app_dir)?;
            Some(vec![format!("/app/{}", bin)])
        }
        Runtime::Hugo => Some(vec!["hugo".into(), "server".into()]),
        Runtime::Java => {
            let jar = find_first_ext(app_dir, "jar")?;
            Some(vec!["java".into(), "-jar".into(), format!("/app/{}", jar)])
        }
        Runtime::Ruby => {
            let entry = find_first_file(app_dir, &["config.ru", "app.rb", "main.rb"])?;
            Some(vec!["ruby".into(), format!("/app/{}", entry)])
        }
        Runtime::Dotnet => {
            let csproj = find_first_ext(app_dir, "csproj")?;
            let name = csproj.trim_end_matches(".csproj");
            Some(vec![
                "dotnet".into(),
                "run".into(),
                "--project".into(),
                format!("/app/{}", name),
            ])
        }
        Runtime::Php => resolve_php_entrypoint(app_dir),
        Runtime::Perl => {
            let entry = find_first_file(app_dir, &["app.pl", "main.pl", "bin/app"])?;
            Some(vec!["perl".into(), format!("/app/{}", entry)])
        }
    }
}

/// Detect the PHP document root and build the built-in server command.
/// Handles: Laravel (artisan), WordPress/OpenEMR (root index.php),
/// `CakePHP` (webroot/), Yii (web/), Slim (public/), and generic fallbacks.
fn resolve_php_entrypoint(app_dir: &Path) -> Option<Vec<String>> {
    // 0. Laravel Octane with RoadRunner — rr binary replaces php -S
    if app_dir.join("rr.yaml").is_file() || app_dir.join(".rr.yaml").is_file() {
        return Some(vec!["rr".into(), "/app".into()]);
    }
    if app_dir.join("artisan").is_file() {
        return Some(server_cmd("/app/public"));
    }

    // 2. Root index.php exists → serve from project root
    //    (WordPress, OpenEMR, Drupal, CodeIgniter, etc.)
    if app_dir.join("index.php").is_file() {
        return Some(server_cmd("/app"));
    }

    // 3. Known web root directories
    const WEB_ROOTS: &[(&str, &str)] = &[
        ("public", "index.php"),
        ("webroot", "index.php"),
        ("web", "index.php"),
        ("htdocs", "index.php"),
        ("www", "index.php"),
    ];
    for (dir, entry) in WEB_ROOTS {
        if app_dir.join(dir).join(entry).is_file() {
            return Some(server_cmd(&format!("/app/{dir}")));
        }
    }

    // 4. Fallback: first index.php in a one-level subdirectory
    if let Ok(entries) = std::fs::read_dir(app_dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let sub = entry.file_name();
                let sub_name = sub.to_string_lossy();
                if sub_name.starts_with('.')
                    || sub_name == "vendor"
                    || sub_name == "node_modules"
                    || sub_name == "tests"
                {
                    continue;
                }
                if entry.path().join("index.php").is_file() {
                    return Some(server_cmd(&format!("/app/{sub_name}")));
                }
            }
        }
    }

    None
}

/// Build `php -S 0.0.0.0:8080 -t <doc_root>` command args.
fn server_cmd(doc_root: &str) -> Vec<String> {
    vec![
        "php".into(),
        "-S".into(),
        "0.0.0.0:8080".into(),
        "-t".into(),
        doc_root.into(),
    ]
}

/// Find the first existing file from a list of candidates.
/// Returns the filename (not full path).
fn find_first_file(dir: &Path, candidates: &[&str]) -> Option<String> {
    for name in candidates {
        if dir.join(name).is_file() {
            return Some((*name).to_string());
        }
    }
    None
}

/// Find the first file with a given extension.
fn find_first_ext(dir: &Path, ext: &str) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        if entry.path().extension().is_some_and(|e| e == ext) {
            return entry.file_name().to_str().map(String::from);
        }
    }
    None
}

/// Find the single native entry binary (ELF or PE `.exe`) in `dir`.
fn find_native_binary(dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file() && is_native_binary(&p) {
            return p.file_name()?.to_str().map(String::from);
        }
    }
    None
}

/// Resolve Node.js entrypoint from package.json or fallback files.
fn find_node_entry(dir: &Path) -> Option<String> {
    // Check package.json "main" or "scripts.start"
    let pkg_path = dir.join("package.json");
    if pkg_path.is_file() {
        if let Ok(contents) = std::fs::read_to_string(&pkg_path) {
            if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&contents) {
                // "main" field
                if let Some(main) = pkg.get("main").and_then(|v| v.as_str()) {
                    return Some(main.to_string());
                }
                // Try "scripts.start": "node server.js" etc.
                if let Some(cmd) = pkg
                    .get("scripts")
                    .and_then(|s| s.get("start"))
                    .and_then(|v| v.as_str())
                {
                    // Extract filename from "node app.js" style commands
                    if let Some(filename) = cmd.split_whitespace().last() {
                        let name = filename.trim_start_matches("./");
                        if dir.join(name).is_file() {
                            return Some(name.to_string());
                        }
                    }
                }
            }
        }
    }
    // Fallback: common entry files
    find_first_file(
        dir,
        &[
            "index.js",
            "app.js",
            "server.js",
            "main.js",
            "server/server.js",
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn detect_python_app() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("app.py"), "print('hi')").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Python));
    }

    #[test]
    fn detect_python_pyproject_only() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname = \"myapp\"",
        )
        .unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Python));
    }

    #[test]
    fn detect_python_setup_py() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("setup.py"), "").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Python));
    }

    #[test]
    fn detect_node_app() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Node));
    }

    #[test]
    fn detect_deno_app() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("deno.json"), "{}").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Deno));
    }

    #[test]
    fn python_beats_node() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("app.py"), "").unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Python));
    }

    #[test]
    fn no_runtime_returns_none() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("README.md"), "hi").unwrap();
        assert_eq!(detect_runtime(dir.path()), None);
    }

    #[test]
    fn php_beats_node_for_laravel() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("composer.json"), "{}").unwrap();
        std::fs::write(dir.path().join("artisan"), "").unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Php));
    }

    #[test]
    fn detect_hugo_app() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("config.toml"), "[server]").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Hugo));
    }

    #[test]
    fn detect_pe_exe_as_binary() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("app.exe"), b"MZ\x90\x00").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Binary));
        assert_eq!(
            resolve_entrypoint(dir.path(), Runtime::Binary),
            Some(vec!["/app/app.exe".into()])
        );
    }

    #[test]
    fn pe_and_elf_is_not_binary() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("app.exe"), b"MZ\x90\x00").unwrap();
        std::fs::write(dir.path().join("app2"), b"\x7fELF\x02\x01").unwrap();
        assert_ne!(detect_runtime(dir.path()), Some(Runtime::Binary));
    }
}
