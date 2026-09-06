//! Pure HTTP request-head parsing shared by the serve loop and the fuzzers.
//!
//! These functions are side-effect free so `daedalus serve`'s request
//! handling can be hammered by the structure-aware fuzzer and by property
//! tests without spawning a process or touching a socket.

use std::io;

/// Locate the end of the request head (first `\r\n\r\n` or bare `\n\n`).
///
/// Returns the byte index just past the terminator, or `None` while the
/// head is still incomplete.
pub fn head_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
        .or_else(|| bytes.windows(2).position(|w| w == b"\n\n").map(|i| i + 2))
}

/// Parse a `Content-Length` declaration from a request head.
///
/// Returns `None` when absent, unparseable, or ambiguous (two different
/// lengths declared), per RFC 7230 §3.3.2 — a duplicated header is a
/// smuggling vector, so ambiguity must be rejected rather than guessed.
/// Obs-fold whitespace before the colon (`Content-Length : 5`) is tolerated.
pub fn content_length(head: &str) -> Option<usize> {
    let mut seen: Option<usize> = None;
    for line in head.lines() {
        let line = line.trim();
        let Some((name, value)) = split_header(line) else {
            continue;
        };
        if !name.eq_ignore_ascii_case("content-length") {
            continue;
        }
        let Ok(value) = value.trim().parse::<usize>() else {
            return None;
        };
        match seen {
            None => seen = Some(value),
            Some(prev) if prev != value => return None,
            Some(_) => {}
        }
    }
    seen
}

/// Split a head line into header name and value at the first colon.
fn split_header(line: &str) -> Option<(&str, &str)> {
    let (name, value) = line.split_once(':')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    Some((name, value))
}

/// Split `path` into path and query string at the first `?`.
pub fn split_path_query(path: &str) -> (&str, &str) {
    match path.split_once('?') {
        Some((p, q)) => (p, q),
        None => (path, ""),
    }
}

/// Parse `application/x-www-form-urlencoded`-style query parameters.
///
/// Pairs with an empty key (`?=value`, `?=x&name=y`) are dropped — a
/// nameless parameter is never meaningful to a handler and its presence
/// breaks canonical re-encoding. Values are NOT percent-decoded — callers
/// validate them against a safe charset and reject anything ambiguous.
pub fn parse_query_params(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        })
        .filter(|(k, _)| !k.is_empty())
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Split and validate a `name[:tag]` artifact spec.
///
/// The tag defaults to `latest`. Names and tags are restricted to
/// `[A-Za-z0-9._-]` so that an alias pushed over HTTP can always be fetched
/// back through `/artifact/<name>:<tag>` with a deterministic split — a
/// colon inside the name would make the round trip asymmetric.
pub fn split_name_tag(spec: &str) -> io::Result<(&str, &str)> {
    let (name, tag) = match spec.rsplit_once(':') {
        Some((n, t)) => (n, t),
        None => (spec, "latest"),
    };
    for part in [name, tag] {
        if part.is_empty() || !safe_alias_chars(part) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid name:tag '{spec}' — use letters, digits, '_', '-', '.' only"),
            ));
        }
    }
    Ok((name, tag))
}

/// Whether `s` only contains characters safe in a registry alias.
fn safe_alias_chars(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::collection::vec as prop_vec;
    use proptest::prelude::*;

    #[test]
    fn head_end_finds_crlf_and_lf() {
        let head = b"GET / HTTP/1.1\r\nHost: x\r\n\r\nXXXX";
        assert_eq!(head_end(head), Some(27));
        let head = b"GET / HTTP/1.1\nHost: x\n\nXXXX";
        assert_eq!(head_end(head), Some(24));
        assert_eq!(head_end(b"GET / HTTP/1.1\r\nHost: x"), None);
        assert_eq!(head_end(b""), None);
    }

    #[test]
    fn content_length_parses_and_rejects_ambiguity() {
        assert_eq!(content_length("GET / HTTP/1.1\r\n\r\n"), None);
        assert_eq!(
            content_length("POST /artifact HTTP/1.1\r\nContent-Length: 42\r\n\r\n"),
            Some(42)
        );
        assert_eq!(content_length("Content-Length : 42"), Some(42));
        assert_eq!(
            content_length("Content-Length: 42\r\nContent-Length: 7"),
            None
        );
        assert_eq!(content_length("Content-Length: potato"), None);
        assert_eq!(
            content_length("Content-Length: 100\r\nContent-Length: 100"),
            Some(100)
        );
    }

    #[test]
    fn query_params_split() {
        assert_eq!(
            parse_query_params("name=a&tag=b"),
            vec![("name".into(), "a".into()), ("tag".into(), "b".into())]
        );
        assert_eq!(
            parse_query_params("name"),
            vec![("name".into(), String::new())]
        );
        assert_eq!(parse_query_params(""), Vec::<(String, String)>::new());
        assert_eq!(
            parse_query_params("a=b=c"),
            vec![("a".into(), "b=c".into())]
        );
        // Nameless parameters are dropped: a handler can never act on them
        // and their presence would break canonical re-encoding.
        assert_eq!(
            parse_query_params("=stray&name=gemma&&&tag=2b&&"),
            vec![("name".into(), "gemma".into()), ("tag".into(), "2b".into())]
        );
    }

    #[test]
    fn split_name_tag_roundtrips() {
        let (n, t) = split_name_tag("ollama:2b").unwrap();
        assert_eq!((n, t), ("ollama", "2b"));
        let (n, t) = split_name_tag("gemma").unwrap();
        assert_eq!((n, t), ("gemma", "latest"));
        assert!(split_name_tag(":latest").is_err());
        assert!(split_name_tag("a:b:c").is_err());
        assert!(split_name_tag("na me").is_err());
        assert!(split_name_tag("a/b").is_err());
        assert!(split_name_tag("ok_name-1.2").is_ok());
    }

    proptest! {
        #[test]
        /// `head_end_bounds` - head_end never escapes the buffer, and a
        /// later scan of a longer buffer cannot report an *earlier* end.
        fn head_end_bounds(
            buf in prop_vec(any::<u8>(), 0..2048),
            tail in prop_vec(any::<u8>(), 0..128),
        ) {
            let combined = [buf.as_slice(), tail.as_slice()].concat();
            if let Some(i) = head_end(&buf) {
                prop_assert!(i <= buf.len());
                prop_assert_eq!(head_end(&combined), Some(i));
            }
            if let Some(i) = head_end(&combined) {
                prop_assert!(i <= combined.len());
            }
        }

        #[test]
        /// `content_length_parseable_ints` - a well-formed numeric header
        /// always parses to exactly the declared value.
        fn content_length_parseable_ints(v in 0usize..=1 << 20) {
            let head = format!("POST /artifact HTTP/1.1\r\nContent-Length: {v}\r\n\r\n");
            prop_assert_eq!(content_length(&head), Some(v));
        }

        #[test]
        /// `split_name_tag_safe_chars` - any spec built from the safe
        /// alphabet round-trips without error.
        fn split_name_tag_safe_chars(
            name in r"[A-Za-z0-9._-]{1,16}",
            tag in r"[A-Za-z0-9._-]{1,16}",
        ) {
            prop_assert!(split_name_tag(&name).is_ok());
            prop_assert!(
                split_name_tag(&format!("{name}:{tag}")).is_ok(),
                "safe spec {:?}:{:?} rejected",
                name,
                tag
            );
        }

        #[test]
        /// `split_name_tag_no_ambiguous_chars` - any spec containing a
        /// character that breaks the GET round trip is rejected before
        /// it can reach the index.
        fn split_name_tag_no_ambiguous_chars(spec in ".*") {
            let ok = split_name_tag(&spec).is_ok();
            let all_safe = spec
                .rsplit_once(':')
                .map(|(n, t)| [n, t])
                .unwrap_or([spec.as_str(), "latest"])
                .iter()
                .all(|p| {
                    !p.is_empty()
                        && p.chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
                });
            prop_assert_eq!(ok, all_safe);
        }
    }
}
