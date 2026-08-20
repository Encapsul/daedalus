//! Runtime detection — identifies which runtime an app directory uses.
//!
//! Detection order matches the Python registry:
//! Python > Deno > Node > Electron > Java > Ruby > .NET > Go > PHP > Perl > Hugo > Wasm > Binary

use std::io::Read;
use std::path::Path;

/// Detected runtime type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    Python,
    Deno,
    Node,
    Electron,
    Java,
    Ruby,
    Dotnet,
    Go,
    Php,
    Perl,
    Hugo,
    Wasm,
    Binary,
}

impl Runtime {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::Deno => "deno",
            Self::Node => "node",
            Self::Electron => "electron",
            Self::Java => "java",
            Self::Ruby => "ruby",
            Self::Dotnet => "dotnet",
            Self::Go => "go",
            Self::Php => "php",
            Self::Perl => "perl",
            Self::Hugo => "hugo",
            Self::Wasm => "wasm",
            Self::Binary => "binary",
        }
    }

    /// Parse a runtime name as written in the binary metadata. Returns `None`
    /// for unknown names so callers can reject crafted metadata instead of
    /// silently falling back to a default interpreter.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "python" => Some(Self::Python),
            "deno" => Some(Self::Deno),
            "node" => Some(Self::Node),
            "electron" => Some(Self::Electron),
            "java" => Some(Self::Java),
            "ruby" => Some(Self::Ruby),
            "dotnet" => Some(Self::Dotnet),
            "go" => Some(Self::Go),
            "php" => Some(Self::Php),
            "perl" => Some(Self::Perl),
            "hugo" => Some(Self::Hugo),
            "wasm" => Some(Self::Wasm),
            "binary" => Some(Self::Binary),
            _ => None,
        }
    }
}

/// Detect the runtime for an app directory by checking marker files.
/// Returns the first match in priority order, preferring runtimes with entry files.
pub fn detect_runtime(app_dir: &Path) -> Option<Runtime> {
    let detected = detect_runtime_candidates(app_dir);

    // Prefer a runtime that has an identifiable entry file.
    for (runtime, _) in &detected {
        let has_entry = match runtime {
            Runtime::Php => {
                app_dir.join("index.php").is_file()
                    || app_dir.join("public/index.php").is_file()
                    || app_dir.join("artisan").is_file()
                    || app_dir.join("entry.php").is_file()
            }
            Runtime::Node => find_node_entry(app_dir).is_some(),
            Runtime::Electron => {
                find_first_file(app_dir, &["main.js", "main.ts", "index.js", "index.ts"]).is_some()
            }
            Runtime::Python => {
                find_first_file(app_dir, &["app.py", "main.py", "__main__.py", "server.py"])
                    .is_some()
            }
            _ => false,
        };
        if has_entry {
            return Some(*runtime);
        }
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
    if detect_electron(dir) {
        candidates.push((Runtime::Electron, true));
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
    if detect_wasm(dir) {
        candidates.push((Runtime::Wasm, true));
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

fn detect_electron(dir: &Path) -> bool {
    let package_json = dir.join("package.json");
    if !package_json.is_file() {
        return false;
    }
    if let Ok(content) = std::fs::read_to_string(package_json) {
        if content.contains("\"electron\"") || content.contains("'electron'") {
            return true;
        }
    }
    false
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

fn detect_wasm(dir: &Path) -> bool {
    dir.join("index.wasm").is_file()
        || dir.join("app.wasm").is_file()
        || dir.join("main.wasm").is_file()
        || dir.extension().is_some_and(|ext| ext == "wasm")
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
///
/// NOTE: intentionally >100 lines because this is the central dispatch for
/// all supported runtimes (Python, Node, PHP, Ruby, Java, .NET, Go, …).
#[allow(clippy::too_many_lines)]
pub fn resolve_entrypoint(app_dir: &Path, runtime: Runtime) -> Option<Vec<String>> {
    match runtime {
        Runtime::Python => {
            // 0. Django manage.py
            if app_dir.join("manage.py").is_file() {
                return Some(vec![
                    "python3".into(),
                    "/app/manage.py".into(),
                    "runserver".into(),
                    "0.0.0.0:8000".into(),
                ]);
            }
            // 1. FastAPI / ASGI: uvicorn or gunicorn with uvicorn worker
            if let Some(asgi) = detect_asgi_entrypoint(app_dir) {
                return Some(asgi);
            }
            // 2. PEP 621 pyproject.toml [project.scripts]
            if let Some(script) = detect_pyproject_scripts(app_dir) {
                return Some(script);
            }
            // 3. python -m module
            if let Some(module) = detect_python_module(app_dir) {
                return Some(vec!["python3".into(), "-m".into(), module]);
            }
            // 4. Script fallback
            let entry =
                find_first_file(app_dir, &["app.py", "main.py", "__main__.py", "server.py"])?;
            Some(vec!["python3".into(), format!("/app/{}", entry)])
        }
        Runtime::Node => {
            // 0. Bun runtime detection
            if app_dir.join("bun.lockb").is_file() || app_dir.join("bunfig.toml").is_file() {
                if let Some(bun_entry) = detect_bun_entry(app_dir) {
                    return Some(bun_entry);
                }
            }
            // 1. Next.js standalone output
            if app_dir
                .join(".next")
                .join("standalone")
                .join("server.js")
                .is_file()
            {
                return Some(vec![
                    "node".into(),
                    "/app/.next/standalone/server.js".into(),
                ]);
            }
            // 2. package.json scripts.start with bun
            if let Some(entry) = find_node_entry(app_dir) {
                if let Some(sub) = entry.strip_prefix("bun ") {
                    return Some(vec!["bun".into(), "run".into(), sub.into()]);
                }
                return Some(vec!["node".into(), format!("/app/{}", entry)]);
            }
            // 3. Common Node entry files
            let entry = find_first_file(
                app_dir,
                &[
                    "bin/www",
                    "dist/main.js",
                    "index.js",
                    "app.js",
                    "server.js",
                    "main.js",
                    "server/server.js",
                ],
            )?;
            Some(vec!["node".into(), format!("/app/{}", entry)])
        }
        Runtime::Electron => {
            let entry = find_first_file(app_dir, &["main.js", "main.ts", "index.js", "index.ts"])
                .or_else(|| find_node_entry(app_dir))?;
            Some(vec!["electron".into(), format!("/app/{}", entry)])
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
        Runtime::Dotnet => {
            // 0. Self-contained publish: look for native executable in publish/ dir
            if let Some(native) = find_dotnet_self_contained(app_dir) {
                return Some(vec![format!("/app/{}", native)]);
            }
            // 1. Framework-dependent: dotnet run --project <csproj>
            let csproj = find_first_ext(app_dir, "csproj")?;
            let name = csproj.trim_end_matches(".csproj");
            Some(vec![
                "dotnet".into(),
                "run".into(),
                "--project".into(),
                format!("/app/{}", name),
            ])
        }
        Runtime::Hugo => Some(vec!["hugo".into(), "server".into()]),
        Runtime::Java => {
            let jar = find_first_ext(app_dir, "jar")?;
            let mut cmd = vec!["java".into(), "-jar".into(), format!("/app/{}", jar)];
            if let Ok(opts) = std::env::var("JAVA_OPTS").or_else(|_| std::env::var("JVM_OPTS")) {
                let opts_vec: Vec<String> =
                    opts.split_whitespace().map(|s| s.to_string()).collect();
                for (i, opt) in opts_vec.iter().enumerate() {
                    cmd.insert(1 + i, opt.clone());
                }
            }
            // Pass the runtime PORT to the JVM as an explicit server.port
            // property. The stub expands `$PORT` from the app environment at
            // launch and drops the flag when PORT is unset (non-web app), so
            // the placeholder must not be resolved at build time.
            cmd.push("-Dserver.port=$PORT".into());
            Some(cmd)
        }
        Runtime::Ruby => {
            // 0. bin/rails (standard Rails)
            if app_dir.join("bin").join("rails").is_file() {
                return Some(vec![
                    "ruby".into(),
                    "/app/bin/rails".into(),
                    "server".into(),
                    "-b".into(),
                    "0.0.0.0".into(),
                ]);
            }
            // 1. Fallback
            let entry = find_first_file(app_dir, &["config.ru", "app.rb", "main.rb"])?;
            Some(vec!["ruby".into(), format!("/app/{}", entry)])
        }
        Runtime::Php => resolve_php_entrypoint(app_dir),
        Runtime::Perl => {
            let entry = find_first_file(app_dir, &["app.pl", "main.pl", "bin/app"])?;
            Some(vec!["perl".into(), format!("/app/{}", entry)])
        }
        Runtime::Wasm => {
            let entry = find_first_file(app_dir, &["index.wasm", "app.wasm", "main.wasm"])?;
            Some(vec!["wasmtime".into(), format!("/app/{}", entry)])
        }
    }
}

/// Detect Python `-m` module entrypoint from `pyproject.toml` or package layout.
/// Returns `Some(module)` if the app should be launched with `python3 -m <module>`.
fn detect_python_module(app_dir: &Path) -> Option<String> {
    if let Ok(content) = std::fs::read_to_string(app_dir.join("pyproject.toml")) {
        if let Ok(pyproject) = content.parse::<toml::Value>() {
            if let Some(tool) = pyproject.get("tool") {
                let has_fastapi = tool.get("fastapi").is_some();
                let entrypoint = tool.get("entrypoint").and_then(|v| v.as_str()).or_else(|| {
                    tool.as_table().and_then(|table| {
                        table
                            .values()
                            .find_map(|v| v.get("entrypoint").and_then(|v| v.as_str()))
                    })
                });
                if has_fastapi || entrypoint.is_some() {
                    if let Some(ep) = entrypoint {
                        let module = ep.split(':').next()?.to_string();
                        if !module.is_empty() {
                            return Some(module);
                        }
                    }
                }
            }
        }
    }
    if app_dir.join("__main__.py").is_file() {
        if let Ok(entries) = std::fs::read_dir(app_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir()
                    && p.join("__init__.py").is_file()
                    && !p.file_name()?.to_str()?.starts_with('.')
                {
                    return p.file_name()?.to_str().map(String::from);
                }
            }
        }
    }
    None
}

/// Detect ASGI entrypoint (uvicorn / gunicorn with uvicorn worker) from
/// `pyproject.toml`, `requirements.txt`, or installed package layout.
/// Returns `Some(argv)` with the full server command.
fn detect_asgi_entrypoint(app_dir: &Path) -> Option<Vec<String>> {
    // Check pyproject.toml for [tool.uvicorn] or gunicorn config
    if let Ok(content) = std::fs::read_to_string(app_dir.join("pyproject.toml")) {
        if let Ok(pyproject) = content.parse::<toml::Value>() {
            if let Some(tool) = pyproject.get("tool") {
                if tool.get("uvicorn").is_some() || tool.get("gunicorn").is_some() {
                    let app = tool.get("app").and_then(|v| v.as_str()).or_else(|| {
                        tool.as_table().and_then(|table| {
                            table
                                .values()
                                .find_map(|v| v.get("app").and_then(|v| v.as_str()))
                        })
                    });
                    if let Some(app_str) = app {
                        let app = app_str.trim().trim_matches('"').to_string();
                        if !app.is_empty() {
                            return Some(vec![
                                "uvicorn".into(),
                                app,
                                "--host".into(),
                                "0.0.0.0".into(),
                                "--port".into(),
                                "8000".into(),
                            ]);
                        }
                    }
                }
            }
        }
    }
    // Check requirements.txt for uvicorn/gunicorn presence
    if let Ok(content) = std::fs::read_to_string(app_dir.join("requirements.txt")) {
        if content.contains("uvicorn") || content.contains("gunicorn") {
            // Find the app module: look for main.py, app.py, or package __main__.py
            let app_module = find_first_file(app_dir, &["main.py", "app.py"])
                .or_else(|| detect_python_module(app_dir).map(|m| format!("{m}:app")))?;
            let module = app_module.strip_suffix(".py").unwrap_or(&app_module);
            return Some(vec![
                "uvicorn".into(),
                format!("{module}:app"),
                "--host".into(),
                "0.0.0.0".into(),
                "--port".into(),
                "8000".into(),
            ]);
        }
    }
    None
}

/// Detect PEP 621 `[project.scripts]` entrypoints from `pyproject.toml`.
/// Returns `Some(argv)` if a `start` or run script is defined.
fn detect_pyproject_scripts(app_dir: &Path) -> Option<Vec<String>> {
    let content = std::fs::read_to_string(app_dir.join("pyproject.toml")).ok()?;
    let mut in_scripts = false;
    let mut start_cmd = None;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("[project.scripts]") {
            in_scripts = true;
            continue;
        }
        if in_scripts && trimmed.starts_with('[') {
            break;
        }
        if in_scripts && trimmed.starts_with("start") {
            if let Some((_, cmd)) = trimmed.split_once('=') {
                start_cmd = Some(cmd.trim().trim_matches('"').to_string());
                break;
            }
        }
    }
    start_cmd.map(|cmd| {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        parts.into_iter().map(|s| s.into()).collect()
    })
}

/// Detect Bun runtime entrypoint from `package.json` or fallback files.
/// Returns `Some(argv)` with bun run command.
fn detect_bun_entry(app_dir: &Path) -> Option<Vec<String>> {
    let pkg_path = app_dir.join("package.json");
    if pkg_path.is_file() {
        if let Ok(contents) = std::fs::read_to_string(&pkg_path) {
            if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&contents) {
                if let Some(cmd) = pkg
                    .get("scripts")
                    .and_then(|s| s.get("start"))
                    .and_then(|v| v.as_str())
                {
                    if cmd.starts_with("bun ") {
                        let sub = cmd.strip_prefix("bun ").unwrap();
                        return Some(vec!["bun".into(), "run".into(), sub.into()]);
                    }
                    // cmd == "bun" (no args) falls through to file-based search below
                }
            }
        }
    }
    find_first_file(app_dir, &["index.ts", "index.js", "main.ts", "main.js"])
        .map(|f| vec!["bun".into(), "run".into(), f])
}

/// Find a self-contained .NET native executable in a publish directory.
/// Returns the relative path if found.
fn find_dotnet_self_contained(app_dir: &Path) -> Option<String> {
    let publish_candidates = [app_dir.join("publish"), app_dir.join("bin").join("Release")];
    for publish_dir in &publish_candidates {
        if !publish_dir.is_dir() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(publish_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() && is_native_binary(&p) {
                    return p
                        .strip_prefix(app_dir)
                        .ok()
                        .and_then(|p| p.to_str().map(String::from));
                }
                if p.is_dir() {
                    if let Ok(sub) = std::fs::read_dir(&p) {
                        for entry in sub.flatten() {
                            let ep = entry.path();
                            if ep.is_file() && is_native_binary(&ep) {
                                return ep
                                    .strip_prefix(app_dir)
                                    .ok()
                                    .and_then(|p| p.to_str().map(String::from));
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Detect the PHP document root and build the built-in server command.
/// Handles: Laravel (artisan), WordPress/OpenEMR (root index.php),
/// `CakePHP` (webroot/), Yii (web/), Slim (public/), `FrankenPHP`, and generic fallbacks.
fn resolve_php_entrypoint(app_dir: &Path) -> Option<Vec<String>> {
    // 0. Laravel Octane with RoadRunner — rr binary replaces php -S
    if app_dir.join("rr.yaml").is_file() || app_dir.join(".rr.yaml").is_file() {
        return Some(vec!["rr".into(), "/app".into()]);
    }
    // 0b. FrankenPHP standalone binary with embedded Laravel
    if app_dir.join("frankenphp").is_file() && app_dir.join("Caddyfile").is_file() {
        return Some(vec!["/app/frankenphp".into(), "php-server".into()]);
    }

    if app_dir.join("artisan").is_file() {
        if app_dir
            .join("vendor")
            .join("laravel")
            .join("octane")
            .is_dir()
            && (app_dir.join("frankenphp").is_file() || app_dir.join("Caddyfile").is_file())
        {
            return Some(vec!["/app/frankenphp".into(), "php-server".into()]);
        }
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

/// Build `php -S 0.0.0.0:<port> -t <doc_root>` command args.
///
/// The listen port defaults to 8080 and can be overridden with `ERE_PHP_PORT`
/// so several erebus PHP apps can share a host without colliding on the same
/// port at runtime.
fn server_cmd(doc_root: &str) -> Vec<String> {
    let port = std::env::var("ERE_PHP_PORT").unwrap_or_else(|_| "8080".to_string());
    vec![
        "php".into(),
        "-S".into(),
        format!("0.0.0.0:{port}"),
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
            "bin/www",
            "dist/main.js",
            ".next/standalone/server.js",
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
    use std::path::PathBuf;
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
    fn detect_electron_app() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies": {"electron": "^30.0.0"}}"#,
        )
        .unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Electron));
    }

    #[test]
    fn detect_electron_requires_dep() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("main.js"), "console.log('hello')").unwrap();
        // Without "electron" in package.json, this is Node, not Electron.
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Node));
    }

    #[test]
    fn electron_entrypoint_uses_main_js() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies": {"electron": "^30.0.0"}}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("main.js"), "console.log('hello')").unwrap();
        assert_eq!(
            resolve_entrypoint(dir.path(), Runtime::Electron),
            Some(vec!["electron".into(), "/app/main.js".into()])
        );
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

    #[test]
    fn detect_wasm_by_filename() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("app.wasm"), b"\x00asm").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Wasm));
    }

    #[test]
    fn detect_wasm_by_extension() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("main.wasm"), b"\x00asm").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Wasm));
    }

    #[test]
    fn detect_wasm_entrypoint() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("index.wasm"), b"\x00asm").unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Wasm);
        assert_eq!(ep, Some(vec!["wasmtime".into(), "/app/index.wasm".into()]));
    }

    #[test]
    fn detect_python_fastapi_module_entrypoint() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            r#"[tool.fastapi]
entrypoint = "app.main:app"
"#,
        )
        .unwrap();
        std::fs::create_dir(dir.path().join("app")).unwrap();
        std::fs::write(dir.path().join("app").join("__init__.py"), "").unwrap();
        std::fs::write(dir.path().join("app").join("main.py"), "").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Python));
        let ep = resolve_entrypoint(dir.path(), Runtime::Python);
        assert_eq!(
            ep,
            Some(vec!["python3".into(), "-m".into(), "app.main".into()])
        );
    }

    #[test]
    fn detect_python_package_main_entrypoint() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("__main__.py"), "print('hi')").unwrap();
        std::fs::create_dir(dir.path().join("mypackage")).unwrap();
        std::fs::write(dir.path().join("mypackage").join("__init__.py"), "").unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Python);
        assert_eq!(
            ep,
            Some(vec!["python3".into(), "-m".into(), "mypackage".into()])
        );
    }

    #[test]
    fn detect_python_script_fallback() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("app.py"), "print('hi')").unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Python);
        assert_eq!(ep, Some(vec!["python3".into(), "/app/app.py".into()]));
    }

    #[test]
    fn detect_django_manage_py() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("manage.py"), "#!/usr/bin/env python\n").unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "Django\n").unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Python);
        assert_eq!(
            ep,
            Some(vec![
                "python3".into(),
                "/app/manage.py".into(),
                "runserver".into(),
                "0.0.0.0:8000".into(),
            ])
        );
    }

    #[test]
    fn detect_fastapi_uvicorn_entrypoint() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "fastapi\nuvicorn\n").unwrap();
        std::fs::write(dir.path().join("main.py"), "app = FastAPI()\n").unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Python);
        assert_eq!(
            ep,
            Some(vec![
                "uvicorn".into(),
                "main:app".into(),
                "--host".into(),
                "0.0.0.0".into(),
                "--port".into(),
                "8000".into(),
            ])
        );
    }

    #[test]
    fn detect_python_pyproject_scripts() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            r#"[project.scripts]
start = "uvicorn main:app"
"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("main.py"), "").unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Python);
        assert_eq!(ep, Some(vec!["uvicorn".into(), "main:app".into()]));
    }

    #[test]
    fn detect_node_express_bin_www() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::create_dir(dir.path().join("bin")).unwrap();
        std::fs::write(dir.path().join("bin").join("www"), "#!/usr/bin/env node\n").unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Node);
        assert_eq!(ep, Some(vec!["node".into(), "/app/bin/www".into()]));
    }

    #[test]
    fn detect_node_nestjs_dist_main() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::create_dir(dir.path().join("dist")).unwrap();
        std::fs::write(dir.path().join("dist").join("main.js"), "console.log('hi')").unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Node);
        assert_eq!(ep, Some(vec!["node".into(), "/app/dist/main.js".into()]));
    }

    #[test]
    fn detect_nextjs_standalone() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::create_dir_all(dir.path().join(".next").join("standalone")).unwrap();
        std::fs::write(
            dir.path()
                .join(".next")
                .join("standalone")
                .join("server.js"),
            "console.log('next')",
        )
        .unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Node);
        assert_eq!(
            ep,
            Some(vec![
                "node".into(),
                "/app/.next/standalone/server.js".into()
            ])
        );
    }

    #[test]
    fn detect_rails_bin_rails() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("Gemfile"), "gem 'rails'\n").unwrap();
        std::fs::create_dir(dir.path().join("bin")).unwrap();
        std::fs::write(
            dir.path().join("bin").join("rails"),
            "#!/usr/bin/env ruby\n",
        )
        .unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Ruby);
        assert_eq!(
            ep,
            Some(vec![
                "ruby".into(),
                "/app/bin/rails".into(),
                "server".into(),
                "-b".into(),
                "0.0.0.0".into(),
            ])
        );
    }

    #[test]
    fn detect_php_frankenphp() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("composer.json"), "{}").unwrap();
        std::fs::write(dir.path().join("artisan"), "#!/usr/bin/env php\n").unwrap();
        std::fs::write(dir.path().join("frankenphp"), "#!/usr/bin/env php\n").unwrap();
        std::fs::write(dir.path().join("Caddyfile"), "{\n\tfrankenphp\n}\n").unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Php);
        assert_eq!(
            ep,
            Some(vec!["/app/frankenphp".into(), "php-server".into()])
        );
    }

    #[test]
    fn detect_php_listen_port_is_env_overridable() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("index.php"), "<?php").unwrap();

        let prev = std::env::var("ERE_PHP_PORT").ok();
        std::env::set_var("ERE_PHP_PORT", "9090");
        let ep = resolve_entrypoint(dir.path(), Runtime::Php);
        match &prev {
            Some(v) => std::env::set_var("ERE_PHP_PORT", v),
            None => std::env::remove_var("ERE_PHP_PORT"),
        }

        let ep = ep.unwrap();
        assert_eq!(ep[0], "php");
        assert_eq!(ep[1], "-S");
        // `php -S addr -t doc_root`: the listen address carries the override.
        assert_eq!(ep[2], "0.0.0.0:9090");
    }

    #[test]
    fn detect_dotnet_self_contained() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("app.csproj"), "<Project />").unwrap();
        std::fs::create_dir_all(dir.path().join("bin").join("Release")).unwrap();
        std::fs::write(
            dir.path().join("bin").join("Release").join("myapp"),
            b"\x7fELF",
        )
        .unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Dotnet);
        let expected = PathBuf::from("/app")
            .join("bin")
            .join("Release")
            .join("myapp");
        assert_eq!(ep, Some(vec![expected.to_string_lossy().into_owned()]));
    }

    #[test]
    fn java_opts_split_into_individual_args() {
        std::env::set_var("JAVA_OPTS", "-Xmx512m -XX:+UseG1GC");
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("app.jar"), b"\x50\x4b\x03\x04").unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Java).unwrap();
        assert_eq!(
            ep,
            vec![
                "java".to_string(),
                "-Xmx512m".to_string(),
                "-XX:+UseG1GC".to_string(),
                "-jar".to_string(),
                "/app/app.jar".to_string(),
                "-Dserver.port=$PORT".to_string(),
            ]
        );
        std::env::remove_var("JAVA_OPTS");
    }

    #[test]
    fn java_entrypoint_emits_port_placeholder() {
        // The `$PORT` placeholder must stay unexpanded at build time — the stub
        // substitutes the run-time PORT value when the app launches.
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("app.jar"), b"\x50\x4b\x03\x04").unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Java).unwrap();
        assert_eq!(ep.last().map(String::as_str), Some("-Dserver.port=$PORT"));
    }

    #[test]
    fn bun_exact_bun_falls_through_to_file_search() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"start":"bun"}}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("index.js"), "console.log('hi')").unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Node);
        assert_eq!(ep, Some(vec!["node".into(), "/app/index.js".into()]));
    }

    #[test]
    fn from_name_roundtrips_all_runtimes() {
        for runtime in [
            Runtime::Python,
            Runtime::Deno,
            Runtime::Node,
            Runtime::Electron,
            Runtime::Java,
            Runtime::Ruby,
            Runtime::Dotnet,
            Runtime::Go,
            Runtime::Php,
            Runtime::Perl,
            Runtime::Hugo,
            Runtime::Wasm,
            Runtime::Binary,
        ] {
            let name = runtime.name();
            assert_eq!(Runtime::from_name(name), Some(runtime), "name: {name}");
        }
        assert_eq!(Runtime::from_name("cobol"), None);
        assert_eq!(Runtime::from_name(""), None);
    }
}
