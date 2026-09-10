//! Runtime-detection fuzzing target: hostile app trees drive `detect_runtime`
//! and `resolve_entrypoint` through the public API.
//!
//! Invariants:
//! - never panics on arbitrary trees/file contents;
//! - resolved entrypoint argv never contains a `..` segment;
//! - Perl detection never resolves a module path outside `app/lib`.

use crate::FuzzTarget;
use anyhow::{bail, Result};
use arbitrary::Unstructured;
use daedalus_core::detect::{detect_runtime, resolve_entrypoint};
use serde::{Deserialize, Serialize};
use std::path::Component;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TreeEntry {
    /// Path relative to the app directory, e.g. `script/x` or `lib/MyApp.pm`.
    name: String,
    /// File contents (ignored when `is_dir`).
    data: String,
    is_dir: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DetectScenario {
    entries: Vec<TreeEntry>,
}

/// Plausible seeds so the corpus starts near interesting layouts instead of
/// requiring pure blind bit-flips to inch toward them.
fn seed_scenario() -> DetectScenario {
    DetectScenario {
        entries: vec![
            TreeEntry {
                name: "script/app".into(),
                data: "use App;\nApp->start;\n".into(),
                is_dir: false,
            },
            TreeEntry {
                name: "lib/App.pm".into(),
                data: "package App;\nuse Mojo::Base -strict;\n".into(),
                is_dir: false,
            },
            TreeEntry {
                name: "app.pl".into(),
                data: "use Mojolicious::Lite;\n".into(),
                is_dir: false,
            },
            TreeEntry {
                name: "Makefile.PL".into(),
                data: "use ExtUtils::MakeMaker;\n".into(),
                is_dir: false,
            },
        ],
    }
}

fn escape_seed() -> DetectScenario {
    DetectScenario {
        entries: vec![
            TreeEntry {
                name: "script/probe".into(),
                data: "use ../../../../etc/passwd;\n".into(),
                is_dir: false,
            },
            TreeEntry {
                name: "script/noise".into(),
                data: "\u{80}\u{81}\u{ff} \u{0}\n".into(),
                is_dir: false,
            },
        ],
    }
}

pub struct DetectFuzzTarget;

impl FuzzTarget for DetectFuzzTarget {
    fn name(&self) -> &'static str {
        "detect"
    }
    fn generate_seed(&self, unstructured: &mut Unstructured) -> Result<Vec<u8>> {
        let scenario = match unstructured.int_in_range(0..=1)? {
            0 => seed_scenario(),
            _ => escape_seed(),
        };
        Ok(serde_json::to_vec(&scenario)?)
    }
    fn mutate(&self, input: &[u8], unstructured: &mut Unstructured) -> Result<Vec<u8>> {
        let mut scenario: DetectScenario = match serde_json::from_slice(input) {
            Ok(s) => s,
            Err(_) => return self.generate_seed(unstructured),
        };
        // Structured tweak: flip an entry between dir/file or corrupt a name.
        if !scenario.entries.is_empty() && unstructured.ratio(1, 2)? {
            let idx = unstructured.int_in_range(0..=scenario.entries.len() - 1)?;
            if unstructured.ratio(1, 3)? {
                scenario.entries[idx].is_dir = !scenario.entries[idx].is_dir;
            } else {
                scenario.entries[idx].name = unstructured.arbitrary::<String>()?;
            }
        }
        if scenario.entries.len() < 8 {
            scenario.entries.push(TreeEntry {
                name: unstructured.arbitrary::<String>()?,
                data: unstructured.arbitrary::<String>()?,
                is_dir: unstructured.ratio(1, 3)?,
            });
        }
        Ok(serde_json::to_vec(&scenario)?)
    }
    fn execute(&self, input: &[u8]) -> Result<()> {
        // Malformed JSON is not a scenario; building the scaffold may fail on
        // fuzzed names (ENAMETOOLONG) — neither is a bug. Only an entrypoint
        // that escapes the app rootfs is an `Err`.
        let Ok(scenario) = serde_json::from_slice::<DetectScenario>(input) else {
            return Ok(());
        };
        let Ok(dir) = tempfile::tempdir() else {
            return Ok(());
        };
        for entry in scenario.entries.iter().take(8) {
            let rel = std::path::PathBuf::from(&entry.name);
            // Never let a `..` segment in the fuzzed name escape the tempdir
            // (the app dir is our sandbox; traversal is exercised via the
            // *module/use* strings inside file content, not entry names).
            if rel.components().any(|c| matches!(c, Component::ParentDir)) {
                continue;
            }
            let path = dir.path().join(&rel);
            if entry.is_dir {
                std::fs::create_dir_all(&path).ok();
            } else if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).ok();
                std::fs::write(&path, entry.data.as_bytes()).ok();
            }
        }
        if let Some(runtime) = detect_runtime(dir.path()) {
            if let Some(argv) = resolve_entrypoint(dir.path(), runtime) {
                for arg in &argv {
                    if arg.split(['/', '\\']).any(|seg| seg == "..") {
                        bail!("entrypoint arg {arg:?} escapes the app rootfs");
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arbitrary::Unstructured;

    #[test]
    fn test_detect_target_creation() {
        let target = DetectFuzzTarget;
        assert_eq!(target.name(), "detect");
        let (_target, _anything) = (target, ());
    }

    #[test]
    fn test_detect_target_executes_seed() {
        let target = DetectFuzzTarget;
        let seed = [0u8; 64];
        let mut u = Unstructured::new(&seed);
        let bytes = target.generate_seed(&mut u).unwrap();
        target.execute(&bytes).unwrap();
    }
}