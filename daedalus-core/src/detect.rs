//! Runtime detection — identifies which runtime an app directory uses.
//!
//! Detection order matches the Python registry:
//! Python > Deno > Node > Electron > Java > Ruby > .NET > Rust > Go > PHP > Perl > Hugo > Wasm > Binary

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
    Rust,
    Go,
    Php,
    Perl,
    Hugo,
    Ollama,
    Wasm,
    Binary,
}

impl Runtime {
    /// `name` - name.
    ///
    /// Description:
    ///
    /// Return: the &'static str
    pub fn name(&self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::Deno => "deno",
            Self::Node => "node",
            Self::Electron => "electron",
            Self::Java => "java",
            Self::Ruby => "ruby",
            Self::Dotnet => "dotnet",
            Self::Rust => "rust",
            Self::Go => "go",
            Self::Php => "php",
            Self::Perl => "perl",
            Self::Hugo => "hugo",
            Self::Ollama => "ollama",
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
            "rust" => Some(Self::Rust),
            "go" => Some(Self::Go),
            "php" => Some(Self::Php),
            "perl" => Some(Self::Perl),
            "hugo" => Some(Self::Hugo),
            "ollama" => Some(Self::Ollama),
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
            Runtime::Rust => app_dir.join("Cargo.toml").is_file(),
            Runtime::Go => {
                app_dir.join("main.go").is_file()
                    || app_dir.join("go.mod").is_file()
                    || app_dir.join("cmd").is_dir()
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
    if detect_ollama(dir) {
        candidates.push((Runtime::Ollama, true));
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
    if detect_rust(dir) {
        candidates.push((Runtime::Rust, true));
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

/// `detect_python` - detect python.
/// `@dir`: directory path
///
/// Description:
///
/// Return: true or false
fn detect_python(dir: &Path) -> bool {
    ["app.py", "main.py", "__main__.py", "server.py"]
        .iter()
        .any(|f| dir.join(f).is_file())
        || dir.join("pyproject.toml").is_file()
        || dir.join("setup.py").is_file()
        || dir.join("requirements.txt").is_file()
        || dir.read_dir().ok().into_iter().flatten().any(|entry| {
            entry
                .ok()
                .map(|e| e.path().extension() == Some("py".as_ref()))
                .unwrap_or(false)
        })
}

/// `has_python_dep` - check whether python dep.
/// `@dir`: directory path
/// `@dep`: dep
///
/// Description:
///
/// Return: true or false
fn has_python_dep(dir: &Path, dep: &str) -> bool {
    if let Ok(content) = std::fs::read_to_string(dir.join("requirements.txt")) {
        return content.lines().any(|line| {
            let name = line.split_whitespace().next().unwrap_or("");
            let name = name
                .split(['>', '<', '=', '!'])
                .next()
                .unwrap_or(name)
                .to_lowercase()
                .replace('-', "_");
            name == dep.replace('-', "_")
        });
    }
    if let Ok(content) = std::fs::read_to_string(dir.join("pyproject.toml")) {
        return content.contains(dep);
    }
    false
}

/// `detect_deno` - detect deno.
/// `@dir`: directory path
///
/// Description:
///
/// Return: true or false
fn detect_deno(dir: &Path) -> bool {
    dir.join("deno.json").is_file() || dir.join("deno.jsonc").is_file()
}

/// `detect_node` - detect node.
/// `@dir`: directory path
///
/// Description:
///
/// Return: true or false
fn detect_node(dir: &Path) -> bool {
    dir.join("package.json").is_file()
}

/// `detect_electron` - detect electron.
/// `@dir`: directory path
///
/// Description:
///
/// Return: true or false
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

/// `detect_java` - detect java.
/// `@dir`: directory path
///
/// Description:
///
/// Return: true or false
fn detect_java(dir: &Path) -> bool {
    dir.join("pom.xml").is_file()
        || dir.join("build.gradle").is_file()
        || dir.join("build.gradle.kts").is_file()
}

/// `detect_ruby` - detect ruby.
/// `@dir`: directory path
///
/// Description:
///
/// Return: true or false
fn detect_ruby(dir: &Path) -> bool {
    dir.join("Gemfile").is_file() || dir.join("_config.yml").is_file()
}

/// `detect_dotnet` - detect dotnet.
/// `@dir`: directory path
///
/// Description:
///
/// Return: true or false
fn detect_dotnet(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .any(|e| e.path().extension().is_some_and(|ext| ext == "csproj"))
        })
        .unwrap_or(false)
}

/// `detect_rust` - detect rust.
/// `@dir`: directory path
///
/// Description:
///
/// Return: true or false
fn detect_rust(dir: &Path) -> bool {
    // Cargo.toml is the authoritative indicator. A Cargo.toml next to a
    // package.json (e.g. Tauri) stays Node by priority — the JS toolchain
    // owns the root manifest there.
    dir.join("Cargo.toml").is_file()
}

/// `detect_go` - detect go.
/// `@dir`: directory path
///
/// Description:
///
/// Return: true or false
fn detect_go(dir: &Path) -> bool {
    // 1. go.mod is the authoritative indicator
    if dir.join("go.mod").is_file() {
        return true;
    }
    // 2. go.sum without go.mod (rare but possible in vendored setups)
    if dir.join("go.sum").is_file() && dir.join("go.work").is_file() {
        return true;
    }
    // 3. main.go in root or cmd/ directory
    if dir.join("main.go").is_file() {
        return true;
    }
    if dir.join("cmd").is_dir() {
        if let Ok(entries) = std::fs::read_dir(dir.join("cmd")) {
            for entry in entries.flatten() {
                if entry.path().join("main.go").is_file() {
                    return true;
                }
            }
        }
    }
    false
}

/// `detect_php` - detect php.
/// `@dir`: directory path
///
/// Description:
///
/// Return: true or false
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

/// `detect_perl` - detect perl.
/// `@dir`: directory path
///
/// Description:
///
/// Return: true or false
fn detect_perl(dir: &Path) -> bool {
    dir.join("Makefile.PL").is_file() || dir.join("cpanfile").is_file()
}

/// `detect_hugo` - detect hugo.
/// `@dir`: directory path
///
/// Description:
///
/// Return: true or false
fn detect_hugo(dir: &Path) -> bool {
    dir.join("config.toml").is_file()
        || dir.join("hugo.toml").is_file()
        || dir.join("config.yaml").is_file()
}

/// Detect an Ollama-based AI app: `ollama` referenced in `package.json` scripts
/// or dependencies, or Ollama model artifacts (`Modelfile`, `models/*.gguf`).
/// GGUF is the container format Ollama uses to bundle a model as a single file.
fn detect_ollama(dir: &Path) -> bool {
    // Node apps that shell out to Ollama declare it as a script/dependency.
    let pkg = dir.join("package.json");
    if pkg.is_file() {
        if let Ok(content) = std::fs::read_to_string(pkg) {
            if content.contains("ollama") {
                return true;
            }
        }
    }
    // Ollama model definitions and model directories.
    if dir.join("Modelfile").is_file() {
        return true;
    }
    let models_have_gguf = match std::fs::read_dir(dir.join("models")) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .any(|entry| entry.path().extension().is_some_and(|ext| ext == "gguf")),
        Err(_) => false,
    };
    // Environment variable overrides.
    let env_ollama = std::env::var("DAEDALUS_OLLAMA").ok()
        .map(|v| v == "1")
        .unwrap_or(false);
    let ollama_host = std::env::var("OLLAMA_HOST").is_ok();
    models_have_gguf || env_ollama || ollama_host
}

/// `detect_wasm` - detect wasm.
/// `@dir`: directory path
///
/// Description:
///
/// Return: true or false
fn detect_wasm(dir: &Path) -> bool {
    dir.join("index.wasm").is_file()
        || dir.join("app.wasm").is_file()
        || dir.join("main.wasm").is_file()
        || dir.extension().is_some_and(|ext| ext == "wasm")
}

/// `detect_binary` - detect binary.
/// `@dir`: directory path
///
/// Description:
///
/// Return: true or false
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
/// `resolve_entrypoint` - resolve entrypoint.
/// `@app_dir`: app dir
/// `@runtime`: runtime
///
/// Description:
///
/// Return: Some(...) if present, None otherwise
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
            // 0b. Streamlit
            if has_python_dep(app_dir, "streamlit") {
                if let Some(entry) = find_first_file(app_dir, &["app.py", "main.py", "server.py"]) {
                    return Some(vec![
                        "streamlit".into(),
                        "run".into(),
                        format!("/app/{}", entry),
                        "--server.port".into(),
                        "8501".into(),
                        "--server.address".into(),
                        "0.0.0.0".into(),
                    ]);
                }
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
                if let Some(sub) = entry.strip_prefix("tsx ") {
                    return Some(vec!["npx".into(), "tsx".into(), sub.into()]);
                }
                if let Some(sub) = entry.strip_prefix("ts-node-dev ") {
                    return Some(vec!["npx".into(), "ts-node-dev".into(), sub.into()]);
                }
                if let Some(sub) = entry.strip_prefix("ts-node ") {
                    return Some(vec!["npx".into(), "ts-node".into(), sub.into()]);
                }
                if let Some(rest) = entry.strip_prefix("node ") {
                    // "node --import tsx src/index.ts" → keep node + args
                    // "node server.js" → keep node + args (relative ok, CWD=/app)
                    return Some(vec!["node".into(), rest.into()]);
                }
                if let Some(rest) = entry.strip_prefix("npx ") {
                    // "npx tsx src/index.ts" → keep npx + args
                    return Some(vec!["npx".into(), rest.into()]);
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
        Runtime::Go | Runtime::Rust | Runtime::Binary => {
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
        Runtime::Ollama => {
            // Serve the Ollama model via the ollama binary so the app's HTTP
            // API is available on its configured port at runtime.
            Some(vec!["ollama".into(), "serve".into()])
        }
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
            // 0. Jekyll (static site generator)
            if app_dir.join("_config.yml").is_file() {
                return Some(vec![
                    "bundle".into(),
                    "exec".into(),
                    "jekyll".into(),
                    "serve".into(),
                    "--host".into(),
                    "0.0.0.0".into(),
                ]);
            }
            // 1. bin/rails (standard Rails)
            if app_dir.join("bin").join("rails").is_file() {
                return Some(vec![
                    "ruby".into(),
                    "/app/bin/rails".into(),
                    "server".into(),
                    "-b".into(),
                    "0.0.0.0".into(),
                ]);
            }
            // 2. Fallback
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
                    if let Some(sub) = cmd.strip_prefix("bun ") {
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
pub fn find_dotnet_self_contained(app_dir: &Path) -> Option<String> {
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
/// The listen port defaults to 8080 and can be overridden with `DAEDALUS_PHP_PORT`
/// so several daedalus PHP apps can share a host without colliding on the same
/// port at runtime.
fn server_cmd(doc_root: &str) -> Vec<String> {
    let port = std::env::var("DAEDALUS_PHP_PORT").unwrap_or_else(|_| "8080".to_string());
    let host = std::env::var("DAEDALUS_PHP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    vec![
        "php".into(),
        "-S".into(),
        format!("{host}:{port}"),
        "-t".into(),
        doc_root.into(),
    ]
}

/// Find the first existing file from a list of candidates.
/// Returns the filename (not full path).
pub fn find_first_file(dir: &Path, candidates: &[&str]) -> Option<String> {
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
/// For monorepos, scans workspace sub-packages to find the actual entry.
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
                    let first_word = cmd.split_whitespace().next().unwrap_or("");
                    // Return full command for TS runners so resolve_entrypoint
                    // can build the correct argv. Only when a file arg exists.
                    let is_ts_runner = matches!(
                        first_word,
                        "tsx" | "ts-node" | "ts-node-dev" | "node" | "bun" | "npx"
                    );
                    let has_file_arg = cmd.split_whitespace().count() > 1;
                    if is_ts_runner && has_file_arg {
                        return Some(cmd.to_string());
                    }
                    // Extract filename from "node app.js" style commands
                    if let Some(filename) = cmd.split_whitespace().last() {
                        let name = filename.trim_start_matches("./");
                        if dir.join(name).is_file() {
                            return Some(name.to_string());
                        }
                    }
                }
                // Monorepo: scan workspace sub-packages
                if let Some(entry) = find_node_workspace_entry(dir, &pkg) {
                    return Some(entry);
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

/// Find entry point in monorepo workspace sub-packages.
/// Handles npm/yarn/pnpm workspaces, Lerna, and Turborepo patterns.
fn find_node_workspace_entry(dir: &Path, pkg: &serde_json::Value) -> Option<String> {
    let patterns: Vec<String> = match pkg.get("workspaces") {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        _ => return None,
    };

    for pattern in patterns {
        // Handle glob patterns like "packages/*"
        if pattern.contains('*') {
            let prefix = pattern.split('*').next()?;
            let search_dir = dir.join(prefix.trim_end_matches('/'));
            if search_dir.is_dir() {
                if let Ok(entries) = std::fs::read_dir(&search_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            // Get relative path from root dir to sub-package
                            let rel = path.strip_prefix(dir).ok()?;
                            if let Some(ep) = find_node_entry_in_subpackage(&path, rel) {
                                return Some(ep);
                            }
                        }
                    }
                }
            }
        } else {
            // Direct path like "packages/api"
            let sub_dir = dir.join(&pattern);
            if sub_dir.is_dir() {
                if let Some(ep) = find_node_entry_in_subpackage(&sub_dir, Path::new(&pattern)) {
                    return Some(ep);
                }
            }
        }
    }
    None
}

/// Find entry point in a single sub-package directory.
/// `rel` is the relative path from root dir to this sub-package (e.g., "packages/api").
fn find_node_entry_in_subpackage(sub_dir: &Path, rel: &Path) -> Option<String> {
    let pkg_path = sub_dir.join("package.json");
    if !pkg_path.is_file() {
        return None;
    }
    let contents = std::fs::read_to_string(&pkg_path).ok()?;
    let pkg: serde_json::Value = serde_json::from_str(&contents).ok()?;

    let rel_str = rel.to_str()?;

    // Check "main" field
    if let Some(main) = pkg.get("main").and_then(|v| v.as_str()) {
        let main_path = sub_dir.join(main);
        if main_path.is_file() {
            return Some(format!("{rel_str}/{main}"));
        }
    }

    // Check "scripts.start" — rewrite command with sub-package prefix
    if let Some(cmd) = pkg
        .get("scripts")
        .and_then(|s| s.get("start"))
        .and_then(|v| v.as_str())
    {
        let first_word = cmd.split_whitespace().next()?;
        let args: Vec<&str> = cmd.split_whitespace().collect();
        if matches!(first_word, "node" | "tsx" | "ts-node" | "npx") && args.len() > 1 {
            // "node dist/main.js" in packages/api → "node packages/api/dist/main.js"
            let file_part = args[1..].join(" ");
            let full_cmd = format!("{first_word} {rel_str}/{file_part}");
            return Some(full_cmd);
        }
    }

    // Fallback: common entry files
    let candidates = ["index.js", "main.js", "src/index.js", "src/main.js"];
    for name in candidates {
        if sub_dir.join(name).is_file() {
            return Some(format!("{rel_str}/{name}"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    /// `detect_python_app` - detect python app.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn detect_python_app() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("app.py"), "print('hi')").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Python));
    }

    #[test]
    /// `detect_python_pyproject_only` - detect python pyproject only.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// `detect_python_setup_py` - detect python setup py.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn detect_python_setup_py() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("setup.py"), "").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Python));
    }

    #[test]
    /// `detect_node_app` - detect node app.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn detect_node_app() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Node));
    }

    #[test]
    /// `detect_deno_app` - detect deno app.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn detect_deno_app() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("deno.json"), "{}").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Deno));
    }

    #[test]
    /// `python_beats_node` - python beats node.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn python_beats_node() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("app.py"), "").unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Python));
    }

    #[test]
    /// `no_runtime_returns_none` - no runtime returns none.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn no_runtime_returns_none() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("README.md"), "hi").unwrap();
        assert_eq!(detect_runtime(dir.path()), None);
    }

    #[test]
    /// `detect_rust_cargo_toml` - detect rust cargo toml.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn detect_rust_cargo_toml() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"hello\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Rust));
    }

    /// A Cargo.toml next to a package.json (Tauri layout) stays Node: the JS
    /// toolchain owns the root manifest there.
    #[test]
    /// `node_beats_rust_when_package_json_present` - node beats rust when package json present.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn node_beats_rust_when_package_json_present() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"a\"\n").unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("index.js"), "").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Node));
    }

    #[test]
    /// `rust_runtime_name_roundtrip` - rust runtime name roundtrip.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn rust_runtime_name_roundtrip() {
        assert_eq!(Runtime::Rust.name(), "rust");
        assert_eq!(Runtime::from_name("rust"), Some(Runtime::Rust));
    }

    /// After `cargo build`, the compiled ELF in the app dir is the entrypoint
    /// (same contract as Go/Binary).
    #[test]
    /// `rust_entrypoint_finds_built_binary` - rust entrypoint finds built binary.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn rust_entrypoint_finds_built_binary() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"a\"\n").unwrap();
        let bin = dir.path().join("hello-rs");
        std::fs::write(&bin, b"\x7fELF fake binary").unwrap();
        assert_eq!(
            resolve_entrypoint(dir.path(), Runtime::Rust),
            Some(vec!["/app/hello-rs".to_string()])
        );
    }

    #[test]
    /// `php_beats_node_for_laravel` - php beats node for laravel.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn php_beats_node_for_laravel() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("composer.json"), "{}").unwrap();
        std::fs::write(dir.path().join("artisan"), "").unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Php));
    }

    #[test]
    /// `detect_hugo_app` - detect hugo app.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn detect_hugo_app() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("config.toml"), "[server]").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Hugo));
    }

    #[test]
    /// `detect_electron_app` - detect electron app.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// `detect_electron_requires_dep` - detect electron requires dep.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn detect_electron_requires_dep() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("main.js"), "console.log('hello')").unwrap();
        // Without "electron" in package.json, this is Node, not Electron.
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Node));
    }

    #[test]
    /// `electron_entrypoint_uses_main_js` - electron entrypoint uses main js.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// `detect_pe_exe_as_binary` - detect pe exe as binary.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// `pe_and_elf_is_not_binary` - pe and elf is not binary.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn pe_and_elf_is_not_binary() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("app.exe"), b"MZ\x90\x00").unwrap();
        std::fs::write(dir.path().join("app2"), b"\x7fELF\x02\x01").unwrap();
        assert_ne!(detect_runtime(dir.path()), Some(Runtime::Binary));
    }

    #[test]
    /// `detect_wasm_by_filename` - detect wasm by filename.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn detect_wasm_by_filename() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("app.wasm"), b"\x00asm").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Wasm));
    }

    #[test]
    /// `detect_wasm_by_extension` - detect wasm by extension.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn detect_wasm_by_extension() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("main.wasm"), b"\x00asm").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Wasm));
    }

    #[test]
    /// `detect_wasm_entrypoint` - detect wasm entrypoint.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn detect_wasm_entrypoint() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("index.wasm"), b"\x00asm").unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Wasm);
        assert_eq!(ep, Some(vec!["wasmtime".into(), "/app/index.wasm".into()]));
    }

    #[test]
    /// `detect_python_fastapi_module_entrypoint` - detect python fastapi module entrypoint.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// `detect_python_package_main_entrypoint` - detect python package main entrypoint.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// `detect_python_script_fallback` - detect python script fallback.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn detect_python_script_fallback() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("app.py"), "print('hi')").unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Python);
        assert_eq!(ep, Some(vec!["python3".into(), "/app/app.py".into()]));
    }

    #[test]
    /// `detect_django_manage_py` - detect django manage py.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// `detect_fastapi_uvicorn_entrypoint` - detect fastapi uvicorn entrypoint.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// `detect_python_pyproject_scripts` - detect python pyproject scripts.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// `detect_node_express_bin_www` - detect node express bin www.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn detect_node_express_bin_www() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::create_dir(dir.path().join("bin")).unwrap();
        std::fs::write(dir.path().join("bin").join("www"), "#!/usr/bin/env node\n").unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Node);
        assert_eq!(ep, Some(vec!["node".into(), "/app/bin/www".into()]));
    }

    #[test]
    /// `detect_node_nestjs_dist_main` - detect node nestjs dist main.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn detect_node_nestjs_dist_main() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::create_dir(dir.path().join("dist")).unwrap();
        std::fs::write(dir.path().join("dist").join("main.js"), "console.log('hi')").unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Node);
        assert_eq!(ep, Some(vec!["node".into(), "/app/dist/main.js".into()]));
    }

    #[test]
    /// `detect_nextjs_standalone` - detect nextjs standalone.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// `detect_rails_bin_rails` - detect rails bin rails.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// `detect_jekyll_entrypoint` - detect jekyll entrypoint.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn detect_jekyll_entrypoint() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("_config.yml"), "title: My Site\n").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Ruby));
        let ep = resolve_entrypoint(dir.path(), Runtime::Ruby);
        assert_eq!(
            ep,
            Some(vec![
                "bundle".into(),
                "exec".into(),
                "jekyll".into(),
                "serve".into(),
                "--host".into(),
                "0.0.0.0".into(),
            ])
        );
    }

    #[test]
    /// `jekyll_beats_rails` - jekyll beats rails.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn jekyll_beats_rails() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("Gemfile"), "gem 'jekyll'\n").unwrap();
        std::fs::write(dir.path().join("_config.yml"), "title: Site\n").unwrap();
        std::fs::create_dir(dir.path().join("bin")).unwrap();
        std::fs::write(
            dir.path().join("bin").join("rails"),
            "#!/usr/bin/env ruby\n",
        )
        .unwrap();
        // Jekyll should be preferred over Rails when _config.yml exists
        let ep = resolve_entrypoint(dir.path(), Runtime::Ruby);
        assert!(ep.unwrap().contains(&"jekyll".to_string()));
    }

    #[test]
    /// `detect_php_frankenphp` - detect php frankenphp.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// `detect_php_listen_port_is_env_overridable` - detect php listen port is env overridable.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn detect_php_listen_port_is_env_overridable() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("index.php"), "<?php").unwrap();

        let prev = std::env::var("DAEDALUS_PHP_PORT").ok();
        std::env::set_var("DAEDALUS_PHP_PORT", "9090");
        let ep = resolve_entrypoint(dir.path(), Runtime::Php);
        match &prev {
            Some(v) => std::env::set_var("DAEDALUS_PHP_PORT", v),
            None => std::env::remove_var("DAEDALUS_PHP_PORT"),
        }

        let ep = ep.unwrap();
        assert_eq!(ep[0], "php");
        assert_eq!(ep[1], "-S");
        // `php -S addr -t doc_root`: the listen address carries the override.
        assert_eq!(ep[2], "127.0.0.1:9090");
    }

    #[test]
    /// `detect_dotnet_self_contained` - detect dotnet self contained.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// `java_opts_split_into_individual_args` - java opts split into individual args.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// `java_entrypoint_emits_port_placeholder` - java entrypoint emits port placeholder.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn java_entrypoint_emits_port_placeholder() {
        // The `$PORT` placeholder must stay unexpanded at build time — the stub
        // substitutes the run-time PORT value when the app launches.
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("app.jar"), b"\x50\x4b\x03\x04").unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Java).unwrap();
        assert_eq!(ep.last().map(String::as_str), Some("-Dserver.port=$PORT"));
    }

    #[test]
    /// `bun_exact_bun_falls_through_to_file_search` - bun exact bun falls through to file search.
    ///
    /// Description:
    ///
    /// Return: nothing
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
    /// `node_tsx_script_start` - node tsx script start.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn node_tsx_script_start() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"start":"tsx src/index.ts"}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src").join("index.ts"), "").unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Node);
        assert_eq!(
            ep,
            Some(vec!["npx".into(), "tsx".into(), "src/index.ts".into()])
        );
    }

    #[test]
    /// `node_ts_node_script_start` - node ts node script start.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn node_ts_node_script_start() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"start":"ts-node src/index.ts"}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src").join("index.ts"), "").unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Node);
        assert_eq!(
            ep,
            Some(vec!["npx".into(), "ts-node".into(), "src/index.ts".into()])
        );
    }

    #[test]
    /// `node_npx_tsx_script_start` - node npx tsx script start.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn node_npx_tsx_script_start() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"start":"npx tsx src/index.ts"}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src").join("index.ts"), "").unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Node);
        assert_eq!(ep, Some(vec!["npx".into(), "tsx src/index.ts".into()]));
    }

    #[test]
    /// `node_node_ts_import_tsx` - node node ts import tsx.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn node_node_ts_import_tsx() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"start":"node --import tsx src/index.ts"}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src").join("index.ts"), "").unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Node);
        assert_eq!(
            ep,
            Some(vec!["node".into(), "--import tsx src/index.ts".into()])
        );
    }

    #[test]
    /// `python_streamlit_entrypoint` - python streamlit entrypoint.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn python_streamlit_entrypoint() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("app.py"), "").unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "streamlit==1.37.0\n").unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Python);
        assert_eq!(
            ep,
            Some(vec![
                "streamlit".into(),
                "run".into(),
                "/app/app.py".into(),
                "--server.port".into(),
                "8501".into(),
                "--server.address".into(),
                "0.0.0.0".into(),
            ])
        );
    }

    #[test]
    /// `python_streamlit_with_hyphen` - python streamlit with hyphen.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn python_streamlit_with_hyphen() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("main.py"), "").unwrap();
        std::fs::write(
            dir.path().join("requirements.txt"),
            "streamlit>=1.0\nfastapi\n",
        )
        .unwrap();
        let ep = resolve_entrypoint(dir.path(), Runtime::Python);
        assert!(ep.unwrap().contains(&"streamlit".to_string()));
    }

    #[test]
    /// `from_name_roundtrips_all_runtimes` - from name roundtrips all runtimes.
    ///
    /// Description:
    ///
    /// Return: nothing
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
            Runtime::Ollama,
            Runtime::Wasm,
            Runtime::Binary,
        ] {
            let name = runtime.name();
            assert_eq!(Runtime::from_name(name), Some(runtime), "name: {name}");
        }
        assert_eq!(Runtime::from_name("cobol"), None);
        assert_eq!(Runtime::from_name(""), None);
    }

    #[test]
    /// `node_monorepo_workspace_glob_pattern` - node monorepo workspace glob pattern.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn node_monorepo_workspace_glob_pattern() {
        let dir = TempDir::new().unwrap();
        // Root package.json with workspaces glob
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"workspaces": ["packages/*"]}"#,
        )
        .unwrap();
        // Create sub-package
        std::fs::create_dir_all(dir.path().join("packages").join("api")).unwrap();
        std::fs::write(
            dir.path().join("packages").join("api").join("package.json"),
            r#"{"main": "index.js"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("packages").join("api").join("index.js"),
            "console.log('api')",
        )
        .unwrap();

        let ep = resolve_entrypoint(dir.path(), Runtime::Node);
        assert_eq!(
            ep,
            Some(vec!["node".into(), "/app/packages/api/index.js".into()])
        );
    }

    #[test]
    /// `node_monorepo_workspace_direct_path` - node monorepo workspace direct path.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn node_monorepo_workspace_direct_path() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"workspaces": ["apps/web"]}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("apps").join("web")).unwrap();
        std::fs::write(
            dir.path().join("apps").join("web").join("package.json"),
            r#"{"main": "server.js"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("apps").join("web").join("server.js"),
            "console.log('web')",
        )
        .unwrap();

        let ep = resolve_entrypoint(dir.path(), Runtime::Node);
        assert_eq!(
            ep,
            Some(vec!["node".into(), "/app/apps/web/server.js".into()])
        );
    }

    #[test]
    /// `node_monorepo_workspace_script_start` - node monorepo workspace script start.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn node_monorepo_workspace_script_start() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"workspaces": ["packages/*"]}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("packages").join("api")).unwrap();
        std::fs::write(
            dir.path().join("packages").join("api").join("package.json"),
            r#"{"scripts": {"start": "node dist/main.js"}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("packages").join("api").join("dist")).unwrap();
        std::fs::write(
            dir.path()
                .join("packages")
                .join("api")
                .join("dist")
                .join("main.js"),
            "console.log('api')",
        )
        .unwrap();

        let ep = resolve_entrypoint(dir.path(), Runtime::Node);
        assert_eq!(
            ep,
            Some(vec!["node".into(), "packages/api/dist/main.js".into()])
        );
    }

    #[test]
    /// `node_monorepo_workspace_string_pattern` - node monorepo workspace string pattern.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn node_monorepo_workspace_string_pattern() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"workspaces": "packages/*"}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("packages").join("cli")).unwrap();
        std::fs::write(
            dir.path().join("packages").join("cli").join("package.json"),
            r#"{"main": "bin/cli.js"}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("packages").join("cli").join("bin")).unwrap();
        std::fs::write(
            dir.path()
                .join("packages")
                .join("cli")
                .join("bin")
                .join("cli.js"),
            "console.log('cli')",
        )
        .unwrap();

        let ep = resolve_entrypoint(dir.path(), Runtime::Node);
        assert_eq!(
            ep,
            Some(vec!["node".into(), "/app/packages/cli/bin/cli.js".into()])
        );
    }

    #[test]
    /// `node_monorepo_workspace_tsx_runner` - node monorepo workspace tsx runner.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn node_monorepo_workspace_tsx_runner() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"workspaces": ["packages/*"]}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("packages").join("web")).unwrap();
        std::fs::write(
            dir.path().join("packages").join("web").join("package.json"),
            r#"{"scripts": {"start": "tsx src/index.ts"}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("packages").join("web").join("src")).unwrap();
        std::fs::write(
            dir.path()
                .join("packages")
                .join("web")
                .join("src")
                .join("index.ts"),
            "",
        )
        .unwrap();

        let ep = resolve_entrypoint(dir.path(), Runtime::Node);
        assert_eq!(
            ep,
            Some(vec![
                "npx".into(),
                "tsx".into(),
                "packages/web/src/index.ts".into()
            ])
        );
    }

    #[test]
    /// `node_monorepo_no_workspaces_falls_through` - node monorepo no workspaces falls through.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn node_monorepo_no_workspaces_falls_through() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("index.js"), "console.log('hi')").unwrap();

        let ep = resolve_entrypoint(dir.path(), Runtime::Node);
        assert_eq!(ep, Some(vec!["node".into(), "/app/index.js".into()]));
    }

    #[test]
    /// `node_monorepo_empty_workspaces_falls_through` - node monorepo empty workspaces falls through.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn node_monorepo_empty_workspaces_falls_through() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"workspaces": []}"#).unwrap();
        std::fs::write(dir.path().join("index.js"), "console.log('hi')").unwrap();

        let ep = resolve_entrypoint(dir.path(), Runtime::Node);
        assert_eq!(ep, Some(vec!["node".into(), "/app/index.js".into()]));
    }

    #[test]
    /// `detect_ollama_modelfile` - detect ollama by Modelfile.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn detect_ollama_modelfile() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("Modelfile"), "FROM llama3.2\n").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Ollama));
    }

    #[test]
    /// `detect_ollama_models_dir` - detect ollama by models dir.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn detect_ollama_models_dir() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join("models")).unwrap();
        std::fs::write(dir.path().join("models").join("model.gguf"), b"GGUF model").unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Ollama));
    }

    #[test]
    /// `detect_ollama_package_json` - detect ollama via package json.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn detect_ollama_package_json() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"start":"ollama serve"},"dependencies":{"ollama":"^0.1.0"}}"#,
        )
        .unwrap();
        assert_eq!(detect_runtime(dir.path()), Some(Runtime::Ollama));
    }

    #[test]
    /// `ollama_runtime_name_roundtrip` - ollama runtime name roundtrip.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn ollama_runtime_name_roundtrip() {
        assert_eq!(Runtime::Ollama.name(), "ollama");
        assert_eq!(Runtime::from_name("ollama"), Some(Runtime::Ollama));
    }
}
