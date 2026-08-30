use daedalus_core::paths::{RemoteBuildCache, RemoteCacheBackend};

use crate::commands::build::BuildArgs;

/// Thin CLI-side wrapper over the core remote cache. The CLI owns the
/// configuration source (`--remote-cache-url`), while the core owns the
/// backend + local fallback logic.
pub fn remote_cache_from_args(
    args: &BuildArgs,
    app_dir: &std::path::Path,
) -> Option<RemoteBuildCache> {
    let url = args.remote_cache_url.as_ref()?;
    let backend = HttpRemoteCache::new(url).ok()?;
    let max_entries = args.remote_cache_max_entries.unwrap_or(50);
    Some(RemoteBuildCache::new(backend, app_dir, max_entries))
}

/// HTTP(S)-backed remote cache.
///
/// GET `{base_url}/{hash}` → 200 with body = artifact, 404 = miss.
/// PUT `{base_url}/{hash}` → 200/201 = stored.
///
/// The base URL must already include any auth/path prefix; this module does
/// not add headers. Use a reverse proxy if you need signing.
pub struct HttpRemoteCache {
    base_url: String,
    client: reqwest::blocking::Client,
}

impl HttpRemoteCache {
    /// new - new.
    /// @base_url: base url
    /// @anyhow: anyhow
    ///
    /// Description:
    ///
    /// Return: Result containing anyhow::Result<Self>
    pub fn new(base_url: impl Into<String>) -> anyhow::Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("daedalus-remote-cache/0.1")
            .build()?;
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            client,
        })
    }
}

impl RemoteCacheBackend for HttpRemoteCache {
    /// get - get.
    /// @hash: hash value
    /// @std: std
    /// @io: io
    ///
    /// Description:
    ///
    /// Return: Result containing std::io::Result<Option<Vec<u8>>>
    fn get(&self, hash: &str) -> std::io::Result<Option<Vec<u8>>> {
        let url = format!("{}/{}", self.base_url, hash);
        let resp = self
            .client
            .get(&url)
            .send()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if resp.status().is_success() {
            let bytes = resp
                .bytes()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            Ok(Some(bytes.to_vec()))
        } else {
            Ok(None)
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
        let url = format!("{}/{}", self.base_url, hash);
        let resp = self
            .client
            .put(&url)
            .body(bytes.to_vec())
            .send()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("remote cache PUT failed: {} {}", resp.status(), url),
            ))
        }
    }
}
