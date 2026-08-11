//! Persistent data directory resolution (`XDG_DATA_HOME` / `~/.local/share` / `~/Library/Application Support`).
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub fn get_persist_dir(app_name: &str) -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        return PathBuf::from(xdg).join("erebus").join(app_name);
    }

    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("erebus")
            .join(app_name);
    }

    PathBuf::from(format!("/tmp/erebus/{app_name}"))
}

pub fn ensure_persist_dir(app_name: &str) -> std::io::Result<PathBuf> {
    let dir = get_persist_dir(app_name);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn get_persist_env(app_name: &str) -> HashMap<String, String> {
    let dir = get_persist_dir(app_name);
    let mut env = HashMap::new();
    env.insert(
        "XBIN_PERSIST_DIR".to_string(),
        dir.to_string_lossy().into_owned(),
    );
    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_get_persist_dir_uses_xdg() {
        let prev = env::var("XDG_DATA_HOME").ok();
        env::set_var("XDG_DATA_HOME", "/custom/xdg");

        let dir = get_persist_dir("myapp");
        assert_eq!(dir, PathBuf::from("/custom/xdg/erebus/myapp"));

        match prev {
            Some(v) => env::set_var("XDG_DATA_HOME", v),
            None => env::remove_var("XDG_DATA_HOME"),
        }
    }

    #[test]
    fn test_get_persist_dir_fallback() {
        let prev_xdg = env::var("XDG_DATA_HOME").ok();
        let prev_home = env::var("HOME").ok();
        env::remove_var("XDG_DATA_HOME");
        env::set_var("HOME", "/home/testuser");

        let dir = get_persist_dir("myapp");
        assert_eq!(dir, PathBuf::from("/home/testuser/.local/share/erebus/myapp"));

        if let Some(v) = prev_xdg {
            env::set_var("XDG_DATA_HOME", v);
        }
        match prev_home {
            Some(v) => env::set_var("HOME", v),
            None => env::remove_var("HOME"),
        }
    }

    #[test]
    fn test_get_persist_env() {
        let prev = env::var("XDG_DATA_HOME").ok();
        env::set_var("XDG_DATA_HOME", "/custom/xdg");

        use std::path::PathBuf;

        let env_map = get_persist_env("myapp");
        let expected = PathBuf::from("/custom/xdg").join("erebus").join("myapp");
        assert_eq!(
            env_map.get("XBIN_PERSIST_DIR").unwrap().as_str(),
            expected.to_string_lossy().as_ref()
        );

        match prev {
            Some(v) => env::set_var("XDG_DATA_HOME", v),
            None => env::remove_var("XDG_DATA_HOME"),
        }
    }
}
