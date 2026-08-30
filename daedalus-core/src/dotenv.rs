//! `.env` file parsing and secret detection for daedalus.
//!
//! Provides `parse_dotenv` (KEY=value parsing with `export` prefix, quote
//! stripping, and comment handling) and `load_dotenv` (resolved against an
//! app directory). Includes secret key detection for security warnings.
use std::collections::HashMap;
use std::fs;
use std::hash::BuildHasher;
use std::io;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

static SECRET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(secret|password|token|api[_-]?key|private[_-]?key|credentials)")
        .expect("valid regex")
});

/// parse_dotenv - parse dotenv.
/// @path: file or directory path
/// @io: io
///
/// Description:
///
/// Return: Result containing io::Result<HashMap<String, String>>
pub fn parse_dotenv(path: &Path) -> io::Result<HashMap<String, String>> {
    let content = fs::read_to_string(path)?;
    let mut env = HashMap::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let line = if let Some(rest) = trimmed.strip_prefix("export ") {
            rest.trim()
        } else {
            trimmed
        };

        if let Some((key, value)) = parse_line(line) {
            env.insert(key, value);
        }
    }

    Ok(env)
}

/// parse_line - parse line.
/// @line: line
///
/// Description:
///
/// Return: Some(...) if present, None otherwise
fn parse_line(line: &str) -> Option<(String, String)> {
    let eq_idx = line.find('=')?;
    let key = line[..eq_idx].trim();
    let raw = line[eq_idx + 1..].trim();

    if key.is_empty() {
        return None;
    }

    let value = strip_quotes(raw);
    Some((key.to_string(), value.clone()))
}

/// strip_quotes - strip quotes.
/// @s: s
///
/// Description:
///
/// Return: the resulting string
fn strip_quotes(s: &str) -> String {
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        return s[1..s.len() - 1].to_string();
    }
    s.to_string()
}

pub fn detect_secret_keys<S: BuildHasher>(env: &HashMap<String, String, S>) -> Vec<String> {
    env.keys()
        .filter(|k| SECRET_RE.is_match(k))
        .cloned()
        .collect()
}

/// load_dotenv - load dotenv.
///
/// Description:
///
/// Return: nothing
pub fn load_dotenv(
    app_dir: &Path,
    env_file: Option<&str>,
    verbose: bool,
) -> HashMap<String, String> {
    let filename = env_file.unwrap_or(".env");
    let path = app_dir.join(filename);

    match parse_dotenv(&path) {
        Ok(env) => {
            if verbose {
                let secrets = detect_secret_keys(&env);
                if !secrets.is_empty() {
                    eprintln!("Warning: .env contains potential secret keys: {secrets:?}");
                }
            }
            env
        }
        Err(_) => {
            if verbose {
                eprintln!("Warning: could not load .env from {}", path.display());
            }
            HashMap::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    /// test_parse_simple - test parse simple.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn test_parse_simple() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "FOO=bar\nBAZ=qux").unwrap();

        let env = parse_dotenv(&path).unwrap();
        assert_eq!(env.get("FOO").unwrap(), "bar");
        assert_eq!(env.get("BAZ").unwrap(), "qux");
    }

    #[test]
    /// test_parse_export_prefix - test parse export prefix.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn test_parse_export_prefix() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "export KEY=value").unwrap();

        let env = parse_dotenv(&path).unwrap();
        assert_eq!(env.get("KEY").unwrap(), "value");
    }

    #[test]
    /// test_parse_quotes - test parse quotes.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn test_parse_quotes() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "D1=\"double\"\nS1='single'").unwrap();

        let env = parse_dotenv(&path).unwrap();
        assert_eq!(env.get("D1").unwrap(), "double");
        assert_eq!(env.get("S1").unwrap(), "single");
    }

    #[test]
    /// test_parse_comments - test parse comments.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn test_parse_comments() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "# comment\nGOOD=yes\n# another\n").unwrap();

        let env = parse_dotenv(&path).unwrap();
        assert_eq!(env.len(), 1);
        assert_eq!(env.get("GOOD").unwrap(), "yes");
    }

    #[test]
    /// test_detect_secrets - test detect secrets.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn test_detect_secrets() {
        let mut env = HashMap::new();
        env.insert("DATABASE_URL".into(), "postgres://...".into());
        env.insert("API_KEY".into(), "abc123".into());
        env.insert("SECRET_TOKEN".into(), "xyz".into());
        env.insert("APP_NAME".into(), "myapp".into());

        let mut secrets = detect_secret_keys(&env);
        secrets.sort();
        assert_eq!(secrets, vec!["API_KEY", "SECRET_TOKEN"]);
    }

    #[test]
    /// test_load_dotenv_not_found - test load dotenv not found.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn test_load_dotenv_not_found() {
        let dir = TempDir::new().unwrap();
        let env = load_dotenv(dir.path(), None, false);
        assert!(env.is_empty());
    }
}
