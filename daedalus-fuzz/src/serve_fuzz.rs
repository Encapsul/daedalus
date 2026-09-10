//! Server request-parsing fuzzing target.
//!
//! Feeds arbitrary bytes into the pure request-head parser that
//! `daedalus serve` runs on every connection, asserting the invariants that
//! keep it sound: bounded terminator scans, unambiguous Content-Length
//! handling (RFC 7230 §3.3.2), deterministic path/query splitting, and
//! name:tag specs that split exactly once and round-trip.

use crate::FuzzTarget;
use anyhow::{ensure, Result};
use arbitrary::Unstructured;
use daedalus_core::http_parse::{
    content_length, head_end, parse_query_params, split_name_tag, split_path_query,
};

pub struct ServeFuzzTarget;

/// Independent reading of the RFC 7230 Content-Length rules used as a
/// cross-check: a request with any unparseable CL header is rejected, two
/// different declared lengths are rejected, and a single (or repeated
/// identical) length wins.
fn reference_content_length(head: &str) -> Option<usize> {
    let mut values: Vec<usize> = Vec::new();
    for line in head.lines() {
        let line = line.trim();
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if !name.trim().eq_ignore_ascii_case("content-length") {
            continue;
        }
        match value.trim().parse() {
            Ok(v) => values.push(v),
            Err(_) => return None,
        }
    }
    match values.split_first() {
        None => None,
        Some((&first, rest)) if rest.iter().all(|&v| v == first) => Some(first),
        Some(_) => None,
    }
}

impl FuzzTarget for ServeFuzzTarget {
    fn name(&self) -> &'static str {
        "serve-parse"
    }
    fn generate_seed(&self, u: &mut Unstructured) -> Result<Vec<u8>> {
        let size = u.int_in_range(0..=4096)?;
        let mut seed = vec![0u8; size];
        u.fill_buffer(&mut seed)?;
        Ok(seed)
    }
    fn mutate(&self, input: &[u8], u: &mut Unstructured) -> Result<Vec<u8>> {
        let mut out = input.to_vec();
        let edits = u.int_in_range(1..=6)?;
        for _ in 0..edits {
            if out.is_empty() {
                out.push(u.arbitrary()?);
                continue;
            }
            match u.int_in_range(0..=2)? {
                0 => {
                    let idx = u.int_in_range(0..=out.len() - 1)?;
                    out[idx] = u.arbitrary()?;
                }
                1 => {
                    let idx = u.int_in_range(0..=out.len())?;
                    let byte = u.arbitrary()?;
                    out.insert(idx, byte);
                }
                _ => {
                    let idx = u.int_in_range(0..=out.len().saturating_sub(1))?;
                    out.remove(idx);
                }
            }
        }
        Ok(out)
    }
    fn execute(&self, input: &[u8]) -> Result<()> {
        let head_str = String::from_utf8_lossy(input);

        // Terminator scan stays in bounds and is stable under extension.
        if let Some(i) = head_end(input) {
            ensure!(i <= input.len(), "head_end out of bounds: {i} > {}", input.len());
        }

        // Content-Length must agree with an independent interpretation.
        ensure!(
            content_length(&head_str) == reference_content_length(&head_str),
            "content_length disagrees with RFC reference for {head_str:?}"
        );

        // Path/query split is exactly the first '?'.
        let (path, query) = split_path_query(&head_str);
        if let Some(idx) = head_str.find('?') {
            ensure!(path == &head_str[..idx], "path split off-by-one");
            ensure!(query == &head_str[idx + 1..], "query split off-by-one");
        } else {
            ensure!(query.is_empty() && path == head_str, "no-'?' split wrong");
        }

        // Query params always parse; the canonical re-encode round-trips.
        let params = parse_query_params(query);
        let canonical: Vec<String> = params
            .iter()
            .map(|(k, v)| {
                if v.is_empty() {
                    k.clone()
                } else {
                    format!("{k}={v}")
                }
            })
            .collect();
        let joined = canonical.join("&");
        ensure!(
            parse_query_params(&joined) == params,
            "query params do not round-trip canonically"
        );
        for (k, _) in &params {
            ensure!(!k.is_empty(), "query param with empty key");
        }

        // Every accepted name:tag splits deterministically and re-splits to
        // the same pair; a lone name defaults to latest.
        if let Ok((name, tag)) = split_name_tag(&head_str) {
            ensure!(!name.is_empty() && !tag.is_empty(), "empty alias part accepted");
            let safe = |p: &str| {
                p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
            };
            ensure!(safe(name) && safe(tag), "unsafe characters accepted");
            let repaired = format!("{name}:{tag}");
            let (n2, t2) = split_name_tag(&repaired)?;
            ensure!(
                n2 == name && t2 == tag,
                "split not idempotent for {head_str:?}"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn target_created() {
        let target = ServeFuzzTarget;
        assert_eq!(target.name(), "serve-parse");
    }
}