use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

#[allow(clippy::cast_precision_loss)]
pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes}B");
    }
    if bytes < 1024 * 1024 {
        return format!("{:.1}KB", bytes as f64 / 1024.0);
    }
    format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
}

pub fn cache_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        PathBuf::from(xdg).join("xbin")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".cache").join("xbin")
    } else {
        PathBuf::from(".xbin").join("cache")
    }
}

pub fn build_cache_dir(app_dir: &Path) -> PathBuf {
    let app_name = app_dir.file_name().unwrap_or_default().to_string_lossy();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    app_name.hash(&mut hasher);
    let hash = hasher.finish();
    cache_dir()
        .join("builds")
        .join(format!("{app_name}_{hash:x}"))
}

/// Intelligent build cache for skipping rebuilds when inputs haven't changed.
///
/// Cache layout: `~/.cache/xbin/builds/<name>_<short_hash>/`
/// Each entry contains: `<combined_hash>.xbin` + `.meta` JSON.
pub struct BuildCache {
    base_dir: PathBuf,
    max_entries: usize,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CacheMeta {
    app_hash: String,
    rt_deps_hash: String,
    timestamp_secs: u64,
}

impl BuildCache {
    #[must_use]
    pub fn new(app_dir: &Path, max_entries: usize) -> Self {
        let app_name = app_dir.file_name().unwrap_or_default().to_string_lossy();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        app_name.hash(&mut hasher);
        let hash = hasher.finish();
        Self {
            base_dir: cache_dir()
                .join("builds")
                .join(format!("{app_name}_{hash:x}")),
            max_entries,
        }
    }

    /// Look up a cached `.xbin` whose `app_hash` and `rt_deps_hash` both match.
    pub fn find(&self, app_hash: &str, rt_deps_hash: &str) -> Option<PathBuf> {
        let entries = self.list_entries()?;
        for entry in entries {
            let meta_path = entry.join(".meta");
            if let Ok(meta) = std::fs::read_to_string(&meta_path) {
                if let Ok(meta) = serde_json::from_str::<CacheMeta>(&meta) {
                    if meta.app_hash == app_hash && meta.rt_deps_hash == rt_deps_hash {
                        let xbin = entry.join("output.xbin");
                        if xbin.is_file() {
                            return Some(xbin);
                        }
                    }
                }
            }
        }
        None
    }

    /// Store a built `.xbin` in the cache.
    pub fn store(
        &self,
        app_hash: &str,
        rt_deps_hash: &str,
        xbin_path: &Path,
    ) -> std::io::Result<()> {
        let combined = format!("{app_hash}_{rt_deps_hash}");
        let entry_dir = self.base_dir.join(&combined);
        std::fs::create_dir_all(&entry_dir)?;

        std::fs::copy(xbin_path, entry_dir.join("output.xbin"))?;

        let meta = CacheMeta {
            app_hash: app_hash.to_string(),
            rt_deps_hash: rt_deps_hash.to_string(),
            timestamp_secs: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        let meta_json = serde_json::to_string(&meta)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(entry_dir.join(".meta"), meta_json)?;

        self.evict_oldest();
        Ok(())
    }

    /// Wipe the entire build cache.
    pub fn clear(&self) -> std::io::Result<()> {
        if self.base_dir.exists() {
            std::fs::remove_dir_all(&self.base_dir)?;
        }
        Ok(())
    }

    fn list_entries(&self) -> Option<Vec<PathBuf>> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&self.base_dir).ok()? {
            let entry = entry.ok()?;
            if entry.path().is_dir() {
                entries.push(entry.path());
            }
        }
        Some(entries)
    }

    fn evict_oldest(&self) {
        let Some(mut entries) = self.list_entries() else {
            return;
        };
        if entries.len() <= self.max_entries {
            return;
        }
        entries.sort_by_key(|e| {
            let meta_path = e.join(".meta");
            std::fs::read_to_string(&meta_path)
                .ok()
                .and_then(|s| serde_json::from_str::<CacheMeta>(&s).ok())
                .map_or(0, |m| m.timestamp_secs)
        });
        while entries.len() > self.max_entries {
            if let Some(oldest) = entries.first() {
                let _ = std::fs::remove_dir_all(oldest);
                entries.remove(0);
            }
        }
    }
}

pub fn default_key_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(xdg).join("xbin").join("keys")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("xbin")
            .join("keys")
    } else {
        PathBuf::from(".xbin").join("keys")
    }
}

pub fn default_trusted_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(xdg).join("xbin").join("trusted-keys")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("xbin")
            .join("trusted-keys")
    } else {
        PathBuf::from(".xbin").join("trusted-keys")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct XdgGuard(Option<String>);

    impl XdgGuard {
        fn new() -> Self {
            let prev = std::env::var("XDG_CACHE_HOME").ok();
            Self(prev)
        }
        fn redirect(path: &Path) {
            std::env::set_var("XDG_CACHE_HOME", path);
        }
    }

    impl Drop for XdgGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
                None => std::env::remove_var("XDG_CACHE_HOME"),
            }
        }
    }

    #[test]
    fn build_cache_store_and_find() {
        let _guard = XdgGuard::new();
        let tmp = tempfile::tempdir().unwrap();
        XdgGuard::redirect(tmp.path());
        let app_dir = tmp.path().join("myapp");
        std::fs::create_dir_all(&app_dir).unwrap();
        let cache = BuildCache::new(&app_dir, 10);

        let fake_xbin = tmp.path().join("fake.xbin");
        std::fs::write(&fake_xbin, b"fake xbin").unwrap();

        cache.store("aaa", "bbb", &fake_xbin).unwrap();
        let found = cache.find("aaa", "bbb");
        assert!(found.is_some());
        assert_eq!(found.unwrap().file_name().unwrap(), "output.xbin");
    }

    #[test]
    fn build_cache_miss() {
        let _guard = XdgGuard::new();
        let tmp = tempfile::tempdir().unwrap();
        XdgGuard::redirect(tmp.path());
        let app_dir = tmp.path().join("myapp");
        std::fs::create_dir_all(&app_dir).unwrap();
        let cache = BuildCache::new(&app_dir, 10);
        assert!(cache.find("xxx", "yyy").is_none());
    }

    #[test]
    fn build_cache_clear() {
        let _guard = XdgGuard::new();
        let tmp = tempfile::tempdir().unwrap();
        XdgGuard::redirect(tmp.path());
        let app_dir = tmp.path().join("myapp");
        std::fs::create_dir_all(&app_dir).unwrap();
        let cache = BuildCache::new(&app_dir, 10);

        let fake_xbin = tmp.path().join("fake.xbin");
        std::fs::write(&fake_xbin, b"fake xbin").unwrap();
        cache.store("aaa", "bbb", &fake_xbin).unwrap();

        cache.clear().unwrap();
        assert!(cache.find("aaa", "bbb").is_none());
    }

    #[test]
    fn build_cache_eviction() {
        let _guard = XdgGuard::new();
        let tmp = tempfile::tempdir().unwrap();
        XdgGuard::redirect(tmp.path());
        let app_dir = tmp.path().join("myapp");
        std::fs::create_dir_all(&app_dir).unwrap();
        let cache = BuildCache::new(&app_dir, 2);

        let fake_xbin = tmp.path().join("fake.xbin");
        std::fs::write(&fake_xbin, b"fake xbin").unwrap();

        cache.store("a1", "b1", &fake_xbin).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        cache.store("a2", "b2", &fake_xbin).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        cache.store("a3", "b3", &fake_xbin).unwrap();

        let entries = cache.list_entries().unwrap();
        assert!(entries.len() <= 2);
    }
}
