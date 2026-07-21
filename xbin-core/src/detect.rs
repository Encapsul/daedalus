//! Runtime detection — identifies which runtime an app directory uses.
//!
//! Detection order matches the Python registry:
//! Python > Deno > Node > Java > Ruby > .NET > Go > PHP > Perl > Binary

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
            entries.filter_map(Result::ok).any(|e| {
                e.path()
                    .extension()
                    .is_some_and(|ext| ext == "csproj")
            })
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
    // Check for a single ELF executable in the directory
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|e| e.path().is_file())
                .filter(|e| {
                    std::fs::read(e.path())
                        .map(|bytes| bytes.len() >= 4 && &bytes[0..4] == b"\x7fELF")
                        .unwrap_or(false)
                })
                .count()
                == 1
        })
        .unwrap_or(false)
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
