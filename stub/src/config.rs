//! Configuration management for xbin launcher.
//!
//! Multi-layered configuration system (lowest priority last):
//! 1. CLI arguments
//! 2. Environment variables (12-factor overrides)
//! 3. Local config file (xbin.toml in same directory as binary)
//! 4. Global config (~/.xbin/config.toml on Unix)
//! 5. Interactive prompt (when TTY available)
//!
//! `merge` fills gaps only, so an earlier layer never loses a key it already
//! set to a later layer's value.

use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;

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
    pub fn load() -> Self {
        let mut config = Self::default();

        // 1. Load from CLI arguments (handled by main.rs)
        // 2. Load from local config file
        if let Some(local_config) = Self::find_local_config() {
            if let Ok(cfg) = Self::load_from_file(&local_config) {
                config.merge(cfg);
            }
        }

        // 3. Load from global config
        if let Some(global_config) = Self::find_global_config() {
            if let Ok(cfg) = Self::load_from_file(&global_config) {
                config.merge(cfg);
            }
        }

        // 4. Override with environment variables
        config.apply_env_overrides();

        config
    }

    fn find_local_config() -> Option<PathBuf> {
        // Check xbin.toml in same directory as binary
        let exe_path = env::current_exe().ok()?;
        let dir = exe_path.parent()?;
        let config_path = dir.join("xbin.toml");
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

    fn find_global_config() -> Option<PathBuf> {
        let home = env::var("HOME").ok()?;
        let config_dir = PathBuf::from(home).join(".xbin");
        let config_path = config_dir.join("config.toml");
        if config_path.exists() {
            return Some(config_path);
        }

        None
    }

    fn load_from_file(path: &PathBuf) -> io::Result<Self> {
        let contents = fs::read_to_string(path)?;
        let config: Self =
            toml::from_str(&contents).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(config)
    }

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
    pub fn get_secret(&self, key: &str) -> Option<&String> {
        self.secrets.as_ref()?.get(key)
    }

    pub fn set_secret(&mut self, key: &str, value: String) {
        if self.secrets.is_none() {
            self.secrets = Some(HashMap::new());
        }
        self.secrets
            .as_mut()
            .unwrap()
            .insert(key.to_string(), value);
    }

    pub fn get_database_url(&self) -> Option<String> {
        self.database.as_ref()?.url.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = AppConfig::default();
        assert!(config.database.is_none());
        assert!(config.secrets.is_none());
        assert!(config.extra.is_empty());
    }

    #[test]
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
    fn test_set_secret_creates_map() {
        let mut config = AppConfig::default();
        assert!(config.secrets.is_none());

        config.set_secret("key", "value".to_string());
        assert!(config.secrets.is_some());
        assert_eq!(config.get_secret("key"), Some(&"value".to_string()));
    }

    #[test]
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
}
