//! Standard paths: cache directory, trusted keys directory, format size helpers.
use std::path::{Path, PathBuf};

#[allow(clippy::cast_precision_loss)]
/// format_size - format size.
/// @bytes: bytes
///
/// Description:
///
/// Return: the resulting string
pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes}B");
    }
    if bytes < 1024 * 1024 {
        return format!("{:.1}KB", bytes as f64 / 1024.0);
    }
    format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
}

/// cache_dir - cache dir.
///
/// Description:
///
/// Return: the PathBuf
pub fn cache_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(local_app_data).join("daedalus");
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
            return PathBuf::from(xdg).join("daedalus");
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(".cache").join("daedalus");
        }
    }
    PathBuf::from(".daedalus").join("cache")
}

/// Simple hash-based build cache.
///
/// Stores `.daedalus` files in `~/.cache/daedalus/builds/` keyed by the SHA-256
/// hex digest of the app source contents **plus** a canonical hash of the
/// build configuration (flags like `--encrypt`/`--squashfs`/`--key` change
/// the output bytes, so they must be part of the key or a cache hit serves
/// a stale/wrong artifact).  Repeated builds of the same source with the
/// same options skip dependency installation, interpreter embedding, and
/// tar creation — the cached binary is copied to the output path instead.
///
/// Cache layout:
/// ```text
/// ~/.cache/daedalus/builds/<app_sha256>-<cfg_sha256>/output.daedalus
/// ~/.cache/daedalus/builds/<app_sha256>-<cfg_sha256>/.meta
/// ~/.cache/daedalus/builds/<app_sha256>-<cfg_sha256>-<target>/output.daedalus   (cross-target)
/// ```
pub struct BuildCache {
    base_dir: PathBuf,
    max_entries: usize,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CacheMeta {
    app_hash: String,
    timestamp_secs: u64,
    ttl_hours: Option<u64>,
}

/// Cache key directory: `app_hash-config_hash`, with `-<target>` appended
/// for cross-target builds so per-arch artifacts never collide.
fn cache_key(app_hash: &str, config_hash: &str, target: Option<&str>) -> String {
    let base = format!("{app_hash}-{config_hash}");
    match target {
        Some(t) => format!("{base}-{t}"),
        None => base,
    }
}

impl BuildCache {
    /// Create a build cache rooted at `~/.cache/daedalus/builds/`.
    ///
    /// `max_entries` caps the number of cached builds; oldest entries are
    /// evicted first when the limit is exceeded.
    #[must_use]
    /// new - new.
    /// @_app_dir:  app dir
    /// @max_entries: max entries
    ///
    /// Description:
    ///
    /// Return: the Self
    pub fn new(_app_dir: &Path, max_entries: usize) -> Self {
        Self {
            base_dir: cache_dir().join("builds"),
            max_entries,
        }
    }

    /// Look up a cached `.daedalus` whose app-hash, config-hash (and target,
    /// for cross builds) match.
    ///
    /// Returns `Some(path)` if a valid cached build exists, `None` otherwise.
    pub fn find(&self, app_hash: &str, config_hash: &str, target: Option<&str>) -> Option<PathBuf> {
        let entry_dir = self.base_dir.join(cache_key(app_hash, config_hash, target));
        let daedalus = entry_dir.join("output.daedalus");
        if daedalus.is_file() {
            Some(daedalus)
        } else {
            None
        }
    }

    /// Store a built `.daedalus` into the cache under the given app hash.
    ///
    /// Cross-target builds pass the target string so a linux and a windows
    /// artifact of the same app never collide under one hash key.
    pub fn store(
        &self,
        app_hash: &str,
        config_hash: &str,
        target: Option<&str>,
        daedalus_path: &Path,
    ) -> std::io::Result<()> {
        let entry_dir = self.base_dir.join(cache_key(app_hash, config_hash, target));
        std::fs::create_dir_all(&entry_dir)?;

        std::fs::copy(daedalus_path, entry_dir.join("output.daedalus"))?;

        let meta = CacheMeta {
            app_hash: app_hash.to_string(),
            timestamp_secs: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            ttl_hours: Some(24),
        };
        let meta_json = serde_json::to_string(&meta)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(entry_dir.join(".meta"), meta_json)?;

        self.evict_oldest();
        Ok(())
    }

    /// Wipe the entire build cache on disk.
    pub fn clear(&self) -> std::io::Result<()> {
        if self.base_dir.exists() {
            std::fs::remove_dir_all(&self.base_dir)?;
        }
        Ok(())
    }

    /// list_entries - list entries.
    ///
    /// Description:
    ///
    /// Return: Some(...) if present, None otherwise
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

    /// evict_oldest - evict oldest.
    ///
    /// Description:
    ///
    /// Return: nothing
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

    /// Garbage-collect cache entries older than their TTL.
    ///
    /// Entries with `ttl_hours` set are removed if their age exceeds that
    /// threshold. Entries without a TTL are kept indefinitely.
    pub fn gc(&self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let Some(entries) = self.list_entries() else {
            return;
        };
        for entry in entries {
            let meta_path = entry.join(".meta");
            let meta = std::fs::read_to_string(&meta_path)
                .ok()
                .and_then(|s| serde_json::from_str::<CacheMeta>(&s).ok());
            if let Some(meta) = meta {
                if let Some(ttl_hours) = meta.ttl_hours {
                    let age_secs = now.saturating_sub(meta.timestamp_secs);
                    let ttl_secs = ttl_hours.saturating_mul(3600);
                    if age_secs > ttl_secs {
                        let _ = std::fs::remove_dir_all(&entry);
                    }
                }
            }
        }
    }
}

/// default_key_dir - get default key dir.
///
/// Description:
///
/// Return: the PathBuf
pub fn default_key_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(xdg).join("daedalus").join("keys")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("daedalus")
            .join("keys")
    } else {
        PathBuf::from(".daedalus").join("keys")
    }
}

/// Directory the stub launcher reads trusted Ed25519 public keys from.
///
/// Mirrors `daedalus-stub`'s `trusted_keys_dir()` exactly so that
/// `daedalus trust` / `daedalus verify` write to and read from the *same* location
/// the launcher checks. Honors `$DAEDALUS_TRUSTED_DIR`; otherwise defaults to
/// `~/.daedalus/trusted-keys/` (home-relative). The home-relative default — not
/// `XDG_DATA_HOME` — is intentional: the stub resolves trust anchors without
/// consulting environment variables that could be spoofed in sandboxed or
/// elevated (`sudo`/setuid) contexts.
pub fn trusted_keys_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("DAEDALUS_TRUSTED_DIR") {
        PathBuf::from(d)
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".daedalus").join("trusted-keys")
    } else {
        PathBuf::from(".daedalus").join("trusted-keys")
    }
}

/// Minimal remote build cache (Depot-style).
///
/// A remote cache stores/retrieves build artifacts by content hash so identical
/// sources can be reused across machines. The interface is intentionally small:
/// `get(hash)` and `put(hash, bytes)`.
///
/// Backends:
/// - `HttpRemoteCache` — GET/PUT against a configurable base URL (in `daedalus-cli`)
/// - `FsRemoteCache` — local directory mirror (useful for testing/proxying)
///
/// URLs encode the hash directly: `{base}/{hash}`. No JSON metadata is needed
/// on the wire because the hash is the content address.
pub trait RemoteCacheBackend: Send + Sync {
    /// get - get.
    /// @hash: hash value
    /// @std: std
    /// @io: io
    ///
    /// Description:
    ///
    /// Return: Result containing std::io::Result<Option<Vec<u8>>>;
    fn get(&self, hash: &str) -> std::io::Result<Option<Vec<u8>>>;
    /// put - put.
    /// @hash: hash value
    /// @bytes: bytes
    /// @std: std
    /// @io: io
    ///
    /// Description:
    ///
    /// Return: Result containing std::io::Result<()>;
    fn put(&self, hash: &str, bytes: &[u8]) -> std::io::Result<()>;
}

/// Local-directory remote cache.
///
/// Stores artifacts under `root/{hash}`. Useful for testing or as a
/// user-space "remote" when the cache directory lives on a network mount.
pub struct FsRemoteCache {
    root: PathBuf,
}

impl FsRemoteCache {
    /// new - new.
    /// @root: root
    ///
    /// Description:
    ///
    /// Return: the Self
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl RemoteCacheBackend for FsRemoteCache {
    /// get - get.
    /// @hash: hash value
    /// @std: std
    /// @io: io
    ///
    /// Description:
    ///
    /// Return: Result containing std::io::Result<Option<Vec<u8>>>
    fn get(&self, hash: &str) -> std::io::Result<Option<Vec<u8>>> {
        let path = self.root.join(hash);
        match std::fs::read(&path) {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// put - put.
    /// @hash: hash value
    /// @bytes: bytes
    /// @std: std
    /// @io: io
    ///
    /// Description:
    ///
    /// Return: Result containing std::io::Result<()>
    fn put(&self, hash: &str, bytes: &[u8]) -> std::io::Result<()> {
        let path = self.root.join(hash);
        std::fs::create_dir_all(&self.root)?;
        std::fs::write(path, bytes)
    }
}

/// Facade over a remote cache backend with local fallback.
///
/// The facade first probes the remote backend; on miss it falls back to the
/// local `BuildCache`. Stores always go to both.
pub struct RemoteBuildCache {
    backend: Box<dyn RemoteCacheBackend>,
    local: BuildCache,
}

impl RemoteBuildCache {
    /// new - new.
    ///
    /// Description:
    ///
    /// Return: nothing
    pub fn new(
        backend: impl RemoteCacheBackend + 'static,
        app_dir: &Path,
        max_entries: usize,
    ) -> Self {
        Self {
            backend: Box::new(backend),
            local: BuildCache::new(app_dir, max_entries),
        }
    }

    /// Try remote first, then local, then build miss.
    ///
    /// On remote hit the artifact is written into the local cache so the
    /// returned path remains valid after this call returns.
    pub fn find(&self, app_hash: &str, config_hash: &str, target: Option<&str>) -> Option<PathBuf> {
        let key = cache_key(app_hash, config_hash, target);
        if let Ok(Some(data)) = self.backend.get(&key) {
            let entry_dir = self.local.base_dir.join(&key);
            if std::fs::create_dir_all(&entry_dir).is_ok() {
                let _ = std::fs::write(entry_dir.join("output.daedalus"), data);
            }
        }
        self.local.find(app_hash, config_hash, target)
    }

    /// Store to both local and remote.
    pub fn store(
        &self,
        app_hash: &str,
        config_hash: &str,
        target: Option<&str>,
        daedalus_path: &Path,
    ) -> std::io::Result<()> {
        let key = cache_key(app_hash, config_hash, target);
        let data = std::fs::read(daedalus_path)?;
        let _ = self.backend.put(&key, &data);
        self.local
            .store(app_hash, config_hash, target, daedalus_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes env-var mutation: parallel tests share the process env, so
    /// `XDG_CACHE_HOME` writes must not race or tests leak each other's dirs.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct XdgGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        prev: Option<String>,
    }

    impl XdgGuard {
        /// new - new.
        ///
        /// Description:
        ///
        /// Return: the Self
        fn new() -> Self {
            let guard = ENV_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let prev = std::env::var("XDG_CACHE_HOME").ok();
            Self { _lock: guard, prev }
        }
        /// redirect - redirect.
        /// @path: file or directory path
        ///
        /// Description:
        ///
        /// Return: nothing
        fn redirect(path: &Path) {
            std::env::set_var("XDG_CACHE_HOME", path);
        }
    }

    impl Drop for XdgGuard {
        /// drop - drop.
        ///
        /// Description:
        ///
        /// Return: nothing
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
                None => std::env::remove_var("XDG_CACHE_HOME"),
            }
        }
    }

    /// RAII guard for an arbitrary env var (saves/restores across a test).
    struct EnvGuard {
        var: &'static str,
        prev: Option<String>,
    }
    impl EnvGuard {
        /// new - new.
        /// @var: var
        ///
        /// Description:
        ///
        /// Return: the Self
        fn new(var: &'static str) -> Self {
            Self {
                var,
                prev: std::env::var(var).ok(),
            }
        }
        /// set - set.
        /// @val: value
        ///
        /// Description:
        ///
        /// Return: nothing
        fn set(&self, val: &str) {
            std::env::set_var(self.var, val);
        }
        /// clear - clear.
        ///
        /// Description:
        ///
        /// Return: nothing
        fn clear(&self) {
            std::env::remove_var(self.var);
        }
    }
    impl Drop for EnvGuard {
        /// drop - drop.
        ///
        /// Description:
        ///
        /// Return: nothing
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var(self.var, v),
                None => std::env::remove_var(self.var),
            }
        }
    }

    #[test]
    /// build_cache_store_and_find - build cache store and find.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn build_cache_store_and_find() {
        let _guard = XdgGuard::new();
        let tmp = tempfile::tempdir().unwrap();
        XdgGuard::redirect(tmp.path());
        let app_dir = tmp.path().join("myapp");
        std::fs::create_dir_all(&app_dir).unwrap();
        let cache = BuildCache::new(&app_dir, 10);

        let fake_daedalus = tmp.path().join("fake.daedalus");
        std::fs::write(&fake_daedalus, b"fake daedalus").unwrap();

        cache.store("aaa", "cfg1", None, &fake_daedalus).unwrap();
        let found = cache.find("aaa", "cfg1", None);
        assert!(found.is_some());
        assert_eq!(found.unwrap().file_name().unwrap(), "output.daedalus");
    }

    #[test]
    /// build_cache_targets_do_not_collide - build cache targets do not collide.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn build_cache_targets_do_not_collide() {
        let _guard = XdgGuard::new();
        let tmp = tempfile::tempdir().unwrap();
        XdgGuard::redirect(tmp.path());
        let app_dir = tmp.path().join("myapp");
        std::fs::create_dir_all(&app_dir).unwrap();
        let cache = BuildCache::new(&app_dir, 10);

        let fake_linux = tmp.path().join("linux.daedalus");
        std::fs::write(&fake_linux, b"linux").unwrap();
        let fake_win = tmp.path().join("win.exe");
        std::fs::write(&fake_win, b"windows").unwrap();

        cache
            .store("aaa", "cfg1", Some("win-x64"), &fake_win)
            .unwrap();
        cache.store("aaa", "cfg1", None, &fake_linux).unwrap();

        assert!(cache.find("aaa", "cfg1", None).is_some());
        assert!(cache.find("aaa", "cfg1", Some("win-x64")).is_some());
        let win = cache.find("aaa", "cfg1", Some("win-x64")).unwrap();
        assert_eq!(std::fs::read_to_string(win).unwrap(), "windows");
    }

    #[test]
    /// build_cache_configs_do_not_collide - build cache configs do not collide.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn build_cache_configs_do_not_collide() {
        let _guard = XdgGuard::new();
        let tmp = tempfile::tempdir().unwrap();
        XdgGuard::redirect(tmp.path());
        let app_dir = tmp.path().join("myapp");
        std::fs::create_dir_all(&app_dir).unwrap();
        let cache = BuildCache::new(&app_dir, 10);

        let plain = tmp.path().join("plain.daedalus");
        std::fs::write(&plain, b"plain").unwrap();
        let encrypted = tmp.path().join("encrypted.daedalus");
        std::fs::write(&encrypted, b"encrypted").unwrap();

        cache.store("aaa", "cfg-plain", None, &plain).unwrap();
        cache.store("aaa", "cfg-encrypt", None, &encrypted).unwrap();

        // Same app hash, different config hash → different artifacts.
        let plain_hit = cache.find("aaa", "cfg-plain", None).unwrap();
        assert_eq!(std::fs::read_to_string(plain_hit).unwrap(), "plain");
        let encrypt_hit = cache.find("aaa", "cfg-encrypt", None).unwrap();
        assert_eq!(std::fs::read_to_string(encrypt_hit).unwrap(), "encrypted");
    }

    #[test]
    /// build_cache_miss - build cache miss.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn build_cache_miss() {
        let _guard = XdgGuard::new();
        let tmp = tempfile::tempdir().unwrap();
        XdgGuard::redirect(tmp.path());
        let app_dir = tmp.path().join("myapp");
        std::fs::create_dir_all(&app_dir).unwrap();
        let cache = BuildCache::new(&app_dir, 10);
        assert!(cache.find("xxx", "cfg1", None).is_none());
    }

    #[test]
    /// build_cache_clear - build cache clear.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn build_cache_clear() {
        let _guard = XdgGuard::new();
        let tmp = tempfile::tempdir().unwrap();
        XdgGuard::redirect(tmp.path());
        let app_dir = tmp.path().join("myapp");
        std::fs::create_dir_all(&app_dir).unwrap();
        let cache = BuildCache::new(&app_dir, 10);

        let fake_daedalus = tmp.path().join("fake.daedalus");
        std::fs::write(&fake_daedalus, b"fake daedalus").unwrap();
        cache.store("aaa", "cfg1", None, &fake_daedalus).unwrap();

        cache.clear().unwrap();
        assert!(cache.find("aaa", "cfg1", None).is_none());
    }

    #[test]
    /// build_cache_eviction - build cache eviction.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn build_cache_eviction() {
        let _guard = XdgGuard::new();
        let tmp = tempfile::tempdir().unwrap();
        XdgGuard::redirect(tmp.path());
        let app_dir = tmp.path().join("myapp");
        std::fs::create_dir_all(&app_dir).unwrap();
        let cache = BuildCache::new(&app_dir, 2);

        let fake_daedalus = tmp.path().join("fake.daedalus");
        std::fs::write(&fake_daedalus, b"fake daedalus").unwrap();

        cache.store("a1", "cfg1", None, &fake_daedalus).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        cache.store("a2", "cfg1", None, &fake_daedalus).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        cache.store("a3", "cfg1", None, &fake_daedalus).unwrap();

        let entries = cache.list_entries().unwrap();
        assert!(entries.len() <= 2);
    }

    #[test]
    /// trusted_keys_dir_matches_stub_default - trusted keys dir matches stub default.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn trusted_keys_dir_matches_stub_default() {
        // $DAEDALUS_TRUSTED_DIR wins; else ~/.daedalus/trusted-keys (home-relative),
        // matching the stub launcher exactly (no XDG resolution).
        let daedalus = EnvGuard::new("DAEDALUS_TRUSTED_DIR");
        let home = EnvGuard::new("HOME");
        daedalus.clear();
        home.set("/fake/home");
        assert_eq!(
            trusted_keys_dir(),
            PathBuf::from("/fake/home/.daedalus/trusted-keys")
        );
        daedalus.set("/custom/keys");
        assert_eq!(trusted_keys_dir(), PathBuf::from("/custom/keys"));
        daedalus.clear();
        assert_eq!(
            trusted_keys_dir(),
            PathBuf::from("/fake/home/.daedalus/trusted-keys")
        );
    }

    #[test]
    /// fs_remote_cache_roundtrip - fs remote cache roundtrip.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn fs_remote_cache_roundtrip() {
        let _guard = XdgGuard::new();
        let tmp = tempfile::tempdir().unwrap();
        XdgGuard::redirect(tmp.path());
        let backend = FsRemoteCache::new(tmp.path());
        let app_dir = tmp.path().join("myapp");
        std::fs::create_dir_all(&app_dir).unwrap();
        let cache = RemoteBuildCache::new(backend, &app_dir, 10);

        let fake = tmp.path().join("artifact.daedalus");
        std::fs::write(&fake, b"remote bytes").unwrap();
        cache.store("h1", "cfg1", None, &fake).unwrap();

        let found = cache.find("h1", "cfg1", None);
        assert!(found.is_some());
        assert_eq!(std::fs::read(found.unwrap()).unwrap(), b"remote bytes");
    }

    #[test]
    /// fs_remote_cache_miss_falls_back_to_local - fs remote cache miss falls back to local.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn fs_remote_cache_miss_falls_back_to_local() {
        let _guard = XdgGuard::new();
        let tmp = tempfile::tempdir().unwrap();
        XdgGuard::redirect(tmp.path());
        let backend = FsRemoteCache::new(tmp.path().join("empty-remote"));
        let app_dir = tmp.path().join("myapp");
        std::fs::create_dir_all(&app_dir).unwrap();
        let cache = RemoteBuildCache::new(backend, &app_dir, 10);

        assert!(cache.find("missing", "cfg1", None).is_none());
    }
}
