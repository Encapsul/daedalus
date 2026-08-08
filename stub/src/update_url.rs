//! Update URL resolution for the xbin launcher stub.
//!
//! Provides `resolve_update_url` (precedence: CLI arg > env > embedded meta)
//! and `normalize_base_url` (strip trailing slashes, reject non-http(s)).

use std::ffi::OsString;
use std::io;

use crate::Metadata;

/// Resolves the update channel base URL:
/// `--xbin-update <URL>` argument > `$XBIN_UPDATE_URL` > embedded `meta.update_url`.
pub fn resolve_update_url(args: &[OsString], idx: usize, meta: &Metadata) -> io::Result<String> {
    if let Some(arg) = args.get(idx) {
        let arg_str = arg.to_string_lossy();
        if arg_str.starts_with("--xbin-update=") {
            let url = arg_str.strip_prefix("--xbin-update=").unwrap_or("");
            if !url.is_empty() {
                return normalize_base_url(url);
            }
        }
    }
    if let Some(next) = args.get(idx + 1) {
        let candidate = next.to_string_lossy();
        if !candidate.starts_with('-') && !candidate.is_empty() {
            return normalize_base_url(&candidate);
        }
    }
    if let Some(url) = std::env::var_os("XBIN_UPDATE_URL") {
        return normalize_base_url(&url.to_string_lossy());
    }
    if let Some(ref url) = meta.update_url {
        return normalize_base_url(url);
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "no update URL — pass one after --xbin-update, set $XBIN_UPDATE_URL, \
         or rebuild with xbin build --update-url",
    ))
}

/// Strips trailing slashes and rejects non-http(s) schemes.
pub fn normalize_base_url(url: &str) -> io::Result<String> {
    let base = url.trim().trim_end_matches('/').to_string();
    if base.is_empty() || !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "update URL must be http(s)://<host>/<path>",
        ));
    }
    Ok(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_base_url_accepts_http_and_https() {
        assert_eq!(
            normalize_base_url("https://updates.example.com/app").unwrap(),
            "https://updates.example.com/app"
        );
        assert_eq!(
            normalize_base_url("http://localhost:8080/updates/").unwrap(),
            "http://localhost:8080/updates"
        );
        assert_eq!(
            normalize_base_url("  https://x.example  ").unwrap(),
            "https://x.example"
        );
    }

    #[test]
    fn normalize_base_url_rejects_unsupported_schemes() {
        assert!(normalize_base_url("ftp://updates.example.com").is_err());
        assert!(normalize_base_url("file:///tmp/updates").is_err());
        assert!(normalize_base_url("").is_err());
        assert!(normalize_base_url("updates.example.com/app").is_err());
    }

    #[test]
    fn resolve_update_url_prefers_the_positional_argument() {
        let args = vec![
            OsString::from("--xbin-update"),
            OsString::from("https://arg.example/app"),
            OsString::from("--flag-for-app"),
        ];
        let meta = Metadata {
            name: "test".into(),
            version: None,
            runtime: String::new(),
            entrypoint: Vec::new(),
            env: std::collections::BTreeMap::new(),
            cwd: None,
            layers: Vec::new(),
            isolation: 0,
            seccomp: false,
            landlock: false,
            services: Vec::new(),
            crypto: None,
            payload_format: String::new(),
            health_check: None,
            update_url: Some("https://embedded.example".into()),
        };
        let base = resolve_update_url(&args, 0, &meta).unwrap();
        assert_eq!(base, "https://arg.example/app");
    }

    #[test]
    fn resolve_update_url_falls_back_to_embedded_metadata() {
        let args = vec![OsString::from("--xbin-update")];
        let meta = Metadata {
            update_url: Some("https://meta.example/app/".into()),
            ..Metadata {
                name: "test".into(),
                version: None,
                runtime: String::new(),
                entrypoint: Vec::new(),
                env: std::collections::BTreeMap::new(),
                cwd: None,
                layers: Vec::new(),
                isolation: 0,
                seccomp: false,
                landlock: false,
                services: Vec::new(),
                crypto: None,
                payload_format: String::new(),
                health_check: None,
                update_url: None,
            }
        };
        let base = resolve_update_url(&args, 0, &meta).unwrap();
        assert_eq!(base, "https://meta.example/app");
    }

    #[test]
    fn resolve_update_url_errors_without_any_source() {
        let args = vec![OsString::from("--xbin-update")];
        let meta = Metadata {
            update_url: None,
            ..Metadata {
                name: "test".into(),
                version: None,
                runtime: String::new(),
                entrypoint: Vec::new(),
                env: std::collections::BTreeMap::new(),
                cwd: None,
                layers: Vec::new(),
                isolation: 0,
                seccomp: false,
                landlock: false,
                services: Vec::new(),
                crypto: None,
                payload_format: String::new(),
                health_check: None,
                update_url: None,
            }
        };
        let err = resolve_update_url(&args, 0, &meta).unwrap_err();
        assert!(err.to_string().contains("no update URL"));
    }

    #[test]
    fn resolve_update_url_handles_equals_syntax() {
        let args = vec![OsString::from("--xbin-update=https://arg.example/app")];
        let meta = Metadata {
            update_url: None,
            ..Metadata {
                name: "test".into(),
                version: None,
                runtime: String::new(),
                entrypoint: Vec::new(),
                env: std::collections::BTreeMap::new(),
                cwd: None,
                layers: Vec::new(),
                isolation: 0,
                seccomp: false,
                landlock: false,
                services: Vec::new(),
                crypto: None,
                payload_format: String::new(),
                health_check: None,
                update_url: Some("https://meta.example/app".into()),
            }
        };
        let base = resolve_update_url(&args, 0, &meta).unwrap();
        assert_eq!(base, "https://arg.example/app");
    }
}
