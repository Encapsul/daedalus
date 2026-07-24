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
            Self::Binary => "binary",
        }
    }
}

/// Detect the runtime for an app directory by checking marker files.
/// Returns the first match in priority order.
pub fn detect_runtime(app_dir: &Path) -> Option<Runtime> {
    if detect_python(app_dir) {
        return Some(Runtime::Python);
    }
    if detect_deno(app_dir) {
        return Some(Runtime::Deno);
    }
    if detect_node(app_dir) {
        return Some(Runtime::Node);
    }
    if detect_java(app_dir) {
        return Some(Runtime::Java);
    }
    if detect_ruby(app_dir) {
        return Some(Runtime::Ruby);
    }
    if detect_dotnet(app_dir) {
        return Some(Runtime::Dotnet);
    }
    if detect_go(app_dir) {
        return Some(Runtime::Go);
    }
    if detect_php(app_dir) {
        return Some(Runtime::Php);
    }
    if detect_perl(app_dir) {
        return Some(Runtime::Perl);
    }
    if detect_binary(app_dir) {
        return Some(Runtime::Binary);
    }
    None
}

fn detect_python(dir: &Path) -> bool {
    ["app.py", "main.py", "__main__.py", "server.py"]
        .iter()
        .any(|f| dir.join(f).is_file())
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
    dir.join("composer.json").is_file()
}

fn detect_perl(dir: &Path) -> bool {
    dir.join("Makefile.PL").is_file() || dir.join("cpanfile").is_file()
}

fn detect_binary(dir: &Path) -> bool {
    let mut elf_count = 0;
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return false,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if std::fs::File::open(&path)
            .ok()
            .and_then(|mut f| {
                let mut buf = [0u8; 4];
                f.read_exact(&mut buf).ok()?;
                Some(&buf[..] == b"\x7fELF")
            })
            .unwrap_or(false)
        {
            elf_count += 1;
            if elf_count > 1 {
                return false;
            }
        }
    }
    elf_count == 1
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
            let elf = find_elf(app_dir)?;
            Some(vec![format!("/app/{}", elf)])
        }
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
        Runtime::Php => {
            let entry = find_first_file(app_dir, &["index.php", "artisan", "public/index.php"])?;
            Some(vec!["php".into(), format!("/app/{}", entry)])
        }
        Runtime::Perl => {
            let entry = find_first_file(app_dir, &["app.pl", "main.pl", "bin/app"])?;
            Some(vec!["perl".into(), format!("/app/{}", entry)])
        }
    }
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

/// Find a single ELF executable in the directory.
fn find_elf(dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file() {
            let mut buf = [0u8; 4];
            if std::fs::File::open(&p).ok()?.read_exact(&mut buf).is_ok() && &buf[..] == b"\x7fELF"
            {
                return p.file_name()?.to_str().map(String::from);
            }
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
}
