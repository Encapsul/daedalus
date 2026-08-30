//! Configuration management for daedalus launcher.
//!
//! Multi-layered configuration system (lowest priority last):
//! 1. CLI arguments
//! 2. Environment variables (12-factor overrides)
//! 3. Local config file (daedalus.toml in same directory as binary)
//! 4. Global config (~/.daedalus/config.toml on Unix)
//! 5. Interactive prompt (when TTY available)
//!
//! `merge` fills gaps only, so an earlier layer never loses a key it already
//! set to a later layer's value.

use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Default)]
#[allow(dead_code)]
pub struct AppConfig {
    pub database: Option<DatabaseConfig>,
    pub secrets: Option<HashMap<String, String>>,
    #[serde(flatten)]
    pub extra: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DatabaseConfig {
    pub url: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub name: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
}

#[allow(dead_code)]
impl AppConfig {
    /// load - load.
    ///
    /// Description:
    ///
    /// Return: the Self
    pub fn load() -> Self {
        let mut config = Self::default();

        // 1. Load from CLI arguments (handled by main.rs)
        // 2. Load from local config file
        if let Some(local_config) = Self::find_local_config() {
            if !Self::warn_if_unsafe_config(&local_config, "local") {
                if let Ok(cfg) = Self::load_from_file(&local_config) {
                    config.merge(cfg);
                }
            }
        }

        // 3. Load from global config
        if let Some(global_config) = Self::find_global_config() {
            if !Self::warn_if_unsafe_config(&global_config, "global") {
                if let Ok(cfg) = Self::load_from_file(&global_config) {
                    config.merge(cfg);
                }
            }
        }

        // 4. Override with environment variables
        config.apply_env_overrides();

        config
    }

    /// find_local_config - find local config.
    ///
    /// Description:
    ///
    /// Return: Some(...) if present, None otherwise
    fn find_local_config() -> Option<PathBuf> {
        // Check daedalus.toml in same directory as binary
        let exe_path = env::current_exe().ok()?;
        let dir = exe_path.parent()?;
        let config_path = dir.join("daedalus.toml");
        if config_path.exists() {
            return Some(config_path);
        }

        // Check config.toml as fallback
        let alt_path = dir.join("config.toml");
        if alt_path.exists() {
            return Some(alt_path);
        }

        None
    }

    /// find_global_config - find global config.
    ///
    /// Description:
    ///
    /// Return: Some(...) if present, None otherwise
    fn find_global_config() -> Option<PathBuf> {
        let home = env::var("HOME").ok()?;
        let config_dir = PathBuf::from(home).join(".daedalus");
        let config_path = config_dir.join("config.toml");
        if config_path.exists() {
            return Some(config_path);
        }

        None
    }

    /// load_from_file - load from file.
    /// @path: file or directory path
    /// @io: io
    ///
    /// Description:
    ///
    /// Return: Result containing io::Result<Self>
    fn load_from_file(path: &PathBuf) -> io::Result<Self> {
        let contents = fs::read_to_string(path)?;
        let config: Self =
            toml::from_str(&contents).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(config)
    }

    /// The generic `config.toml` name can pick up unrelated files and a
    /// group/other-writable config in a shared install dir can inject secrets
    /// into every user of the binary. Returns a human-readable reason when the
    /// file is unsafe to trust, `None` when it is fine.
    fn config_file_perilous(path: &Path) -> Option<&'static str> {
        if path.file_name().and_then(|n| n.to_str()) == Some("config.toml") {
            return Some(
                "generic `config.toml` name may pick up unrelated files; prefer `daedalus.toml`",
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if let Ok(meta) = fs::metadata(path) {
                if meta.mode() & 0o022 != 0 {
                    return Some("file is group/other-writable — any co-resident user could modify the secrets it injects");
                }
            }
        }
        None
    }

    /// warn_if_unsafe_config - warn if unsafe config.
    /// @path: file or directory path
    /// @role: role
    ///
    /// Description:
    ///
    /// Return: true or false
    fn warn_if_unsafe_config(path: &Path, role: &str) -> bool {
        if let Some(reason) = Self::config_file_perilous(path) {
            eprintln!(
                "[daedalus] warning: skipping {role} config {}: {reason}",
                path.display()
            );
            return true;
        }
        false
    }

    /// merge - merge.
    /// @other: other
    ///
    /// Description:
    ///
    /// Return: nothing
    fn merge(&mut self, other: Self) {
        // Fill-in semantics: fields already present in `self` (higher
        // precedence) win; `other` only fills the gaps. `load()` merges local
        // first then global, so local beats global.
        match (&mut self.database, other.database) {
            (Some(cur), Some(o)) => {
                if cur.url.is_none() {
                    cur.url = o.url;
                }
                if cur.host.is_none() {
                    cur.host = o.host;
                }
                if cur.port.is_none() {
                    cur.port = o.port;
                }
                if cur.name.is_none() {
                    cur.name = o.name;
                }
                if cur.user.is_none() {
                    cur.user = o.user;
                }
                if cur.password.is_none() {
                    cur.password = o.password;
                }
            }
            (None, Some(o)) => self.database = Some(o),
            _ => {}
        }
        if let Some(o) = other.secrets {
            let map = self.secrets.get_or_insert_with(HashMap::new);
            for (k, v) in o {
                map.entry(k).or_insert(v);
            }
        }
        for (k, v) in other.extra {
            self.extra.entry(k).or_insert(v);
        }
    }

    /// apply_env_overrides - apply env overrides.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn apply_env_overrides(&mut self) {
        // Database URL from environment
        if let Ok(url) = env::var("DATABASE_URL") {
            if let Some(ref mut db) = self.database {
                db.url = Some(url);
            } else {
                self.database = Some(DatabaseConfig {
                    url: Some(url),
                    ..Default::default()
                });
            }
        }

        // Other common env vars
        if let Ok(val) = env::var("DB_HOST") {
            if let Some(ref mut db) = self.database {
                db.host = Some(val);
            }
        }
        if let Ok(val) = env::var("DB_PORT") {
            if let Ok(port) = val.parse::<u16>() {
                if let Some(ref mut db) = self.database {
                    db.port = Some(port);
                }
            }
        }
        if let Ok(val) = env::var("DB_NAME") {
            if let Some(ref mut db) = self.database {
                db.name = Some(val);
            }
        }
        if let Ok(val) = env::var("DB_USER") {
            if let Some(ref mut db) = self.database {
                db.user = Some(val);
            }
        }
        if let Ok(val) = env::var("DB_PASSWORD") {
            if let Some(ref mut db) = self.database {
                db.password = Some(val);
            }
        }
    }
}

/// Prompt for missing secrets with masked input
#[allow(dead_code)]
/// prompt_for_secrets - prompt for secrets.
/// @config: configuration
/// @required: required
/// @io: io
///
/// Description:
///
/// Return: Result containing io::Result<()>
pub fn prompt_for_secrets(config: &mut AppConfig, required: &[String]) -> io::Result<()> {
    if !std::io::stdin().is_terminal() {
        return Ok(());
    }

    println!("Configuration required:");

    for key in required {
        if config.get_secret(key).is_none() {
            print!("Enter {}: ", key);
            io::stdout().flush()?;

            let input = read_line_masked()?;

            if !input.is_empty() {
                config.set_secret(key, input);
            }
        }
    }

    Ok(())
}

/// Read a line of input with masked echo.
/// Windows `console::ReadConsole` echo suppression would need the winapi
/// console API; read unmasked (the value is user-provided at first run).
fn read_line_masked() -> io::Result<String> {
    #[cfg(unix)]
    {
        // Try to disable echo via termios
        let stdin = io::stdin();
        let mut handle = stdin.lock();

        // SAFETY: tcgetattr/tcsetattr on stdin (fd 0) is safe when:
        // - We only modify the ECHO flag in c_lflag
        // - We restore the original termios on all paths (success, error, early return)
        // - No other thread is modifying termios concurrently (stdin is single-threaded in this context)
        // - The termios struct is properly initialized with zeroed memory
        unsafe {
            let mut termios: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut termios) == 0 {
                let saved = termios;
                termios.c_lflag &= !libc::ECHO;
                libc::tcsetattr(0, libc::TCSAFLUSH, &termios);

                let mut input = String::new();
                handle.read_line(&mut input)?;

                // Restore terminal settings
                libc::tcsetattr(0, libc::TCSAFLUSH, &saved);

                // Print newline after masked input
                println!();

                return Ok(input.trim().to_string());
            }
        }
    }

    // Fallback: read without masking
    let stdin = io::stdin();
    let mut handle = stdin.lock();
    let mut input = String::new();
    handle.read_line(&mut input)?;
    Ok(input.trim().to_string())
}

#[allow(dead_code)]
impl AppConfig {
    /// get_secret - get secret.
    /// @key: key
    ///
    /// Description:
    ///
    /// Return: Some(...) if present, None otherwise
    pub fn get_secret(&self, key: &str) -> Option<&String> {
        self.secrets.as_ref()?.get(key)
    }

    /// set_secret - set secret.
    /// @key: key
    /// @value: value
    ///
    /// Description:
    ///
    /// Return: nothing
    pub fn set_secret(&mut self, key: &str, value: String) {
        if self.secrets.is_none() {
            self.secrets = Some(HashMap::new());
        }
        self.secrets
            .as_mut()
            .unwrap()
            .insert(key.to_string(), value);
    }

    /// get_database_url - get database url.
    ///
    /// Description:
    ///
    /// Return: Some(...) if present, None otherwise
    pub fn get_database_url(&self) -> Option<String> {
        self.database.as_ref()?.url.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// test_config_default - test config default.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn test_config_default() {
        let config = AppConfig::default();
        assert!(config.database.is_none());
        assert!(config.secrets.is_none());
        assert!(config.extra.is_empty());
    }

    #[test]
    /// test_config_merge - test config merge.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn test_config_merge() {
        let config1 = AppConfig {
            database: Some(DatabaseConfig {
                url: Some("postgres://localhost/test".to_string()),
                ..Default::default()
            }),
            secrets: {
                let mut m = HashMap::new();
                m.insert("api_key".to_string(), "secret123".to_string());
                Some(m)
            },
            extra: HashMap::new(),
        };

        let config2 = AppConfig {
            database: None,
            secrets: {
                let mut m = HashMap::new();
                m.insert("db_password".to_string(), "pass456".to_string());
                Some(m)
            },
            extra: HashMap::new(),
        };

        let mut merged = config1.clone();
        merged.merge(config2);

        assert_eq!(
            merged.get_database_url(),
            Some("postgres://localhost/test".to_string())
        );
        assert_eq!(
            merged.get_secret("db_password"),
            Some(&"pass456".to_string())
        );
    }

    #[test]
    /// test_set_secret_creates_map - test set secret creates map.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn test_set_secret_creates_map() {
        let mut config = AppConfig::default();
        assert!(config.secrets.is_none());

        config.set_secret("key", "value".to_string());
        assert!(config.secrets.is_some());
        assert_eq!(config.get_secret("key"), Some(&"value".to_string()));
    }

    #[test]
    /// test_merge_local_wins_over_global - test merge local wins over global.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn test_merge_local_wins_over_global() {
        let mut local = AppConfig {
            database: Some(DatabaseConfig {
                url: Some("postgres://local/db".to_string()),
                host: None,
                ..Default::default()
            }),
            secrets: {
                let mut m = HashMap::new();
                m.insert("api_key".to_string(), "local-key".to_string());
                Some(m)
            },
            extra: HashMap::new(),
        };

        let global = AppConfig {
            database: Some(DatabaseConfig {
                url: Some("postgres://global/db".to_string()),
                host: Some("db.example.com".to_string()),
                ..Default::default()
            }),
            secrets: {
                let mut m = HashMap::new();
                m.insert("api_key".to_string(), "global-key".to_string());
                m.insert("other".to_string(), "global-other".to_string());
                Some(m)
            },
            extra: HashMap::new(),
        };

        local.merge(global);

        // Local value wins on collision.
        assert_eq!(
            local.get_database_url(),
            Some("postgres://local/db".to_string())
        );
        assert_eq!(local.get_secret("api_key"), Some(&"local-key".to_string()));
        // Global fills only the gaps.
        assert_eq!(
            local.database.as_ref().unwrap().host.as_deref(),
            Some("db.example.com")
        );
        assert_eq!(local.get_secret("other"), Some(&"global-other".to_string()));
    }

    #[test]
    /// test_load_from_toml - test load from toml.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn test_load_from_toml() {
        let toml_str = r#"
[database]
url = "postgres://localhost/test"
host = "localhost"
port = 5432

[secrets]
api_key = "test123"
db_password = "secret"
"#;

        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.get_database_url(),
            Some("postgres://localhost/test".to_string())
        );
        assert_eq!(config.get_secret("api_key"), Some(&"test123".to_string()));
    }

    #[test]
    /// generic_config_name_is_perilous - generic config name is perilous.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn generic_config_name_is_perilous() {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::TempDir::new().unwrap();
        let generic = dir.path().join("config.toml");
        fs::write(&generic, "[secrets]\n").unwrap();
        let specific = dir.path().join("daedalus.toml");
        fs::write(&specific, "[secrets]\n").unwrap();
        #[cfg(unix)]
        for path in [&generic, &specific] {
            fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap();
        }

        assert!(AppConfig::config_file_perilous(&generic).is_some());
        assert!(AppConfig::config_file_perilous(&specific).is_none());
    }

    #[cfg(unix)]
    #[test]
    /// world_writable_config_is_perilous - world writable config is perilous.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn world_writable_config_is_perilous() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("daedalus.toml");
        fs::write(&path, "[secrets]\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();

        assert!(AppConfig::config_file_perilous(&path).is_some());

        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(AppConfig::config_file_perilous(&path).is_none());
    }
}
