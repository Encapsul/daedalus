//! Hidden-dependency analysis ("AI analyzer" in the roadmap sense).
//!
//! Detects external executables an app invokes at runtime through
//! subprocess/exec/dlopen calls. Such dependencies are invisible to static
//! link analysis: an app may shell out to `ffmpeg` or dlopen `libfoo.so`, and
//! neither is discoverable from its declared manifest. This module is a
//! heuristic scanner (pattern matching, no model, zero external runtime deps)
//! that deliberately favors recall over precision — a false positive is cheap
//! (a packager eyeballs the list), a missed dependency breaks the deployed
//! binary.
//!
//! The scan is reporting-only today; packaging integration is a later step.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::detect::Runtime;

/// Directories never descended into. Huge or generated, they would dominate
/// scan time and produce noise from vendored/compiled artifacts.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "__pycache__",
    ".venv",
    "venv",
    "target",
    "dist",
    "build",
];

/// Source extensions whose contents are scanned for executable references.
/// Only text files in these languages are worth scanning; everything else is
/// either not source or not likely to spawn subprocesses.
const SOURCE_EXTS: &[&str] = &[
    "py", "js", "ts", "jsx", "tsx", "mjs", "cjs", "sh", "rb", "php", "go", "rs", "java", "pl",
    "lua",
];

/// Well-known external binaries flagged regardless of call context. Centralized
/// so coverage decisions live in one place and the per-line scanner stays lean.
const KNOWN_BINARIES: &[&str] = &[
    "ffmpeg",
    "ffprobe",
    "curl",
    "wget",
    "git",
    "node",
    "npm",
    "python3",
    "python",
    "java",
    "openssl",
    "convert",
    "magick",
    "gs",
    "ghostscript",
    "pandoc",
    "wkhtmltopdf",
    "sqlite3",
    "redis-cli",
    "mysql",
    "psql",
];

/// Hard caps that bound work on pathological directories.
const MAX_DEPTH: u32 = 16;
const MAX_FILES: usize = 20_000;
const MAX_BYTES: u64 = 100 * 1024 * 1024;

/// Matches a quoted executable token following a subprocess/exec/dlopen call.
/// The first quoted string after the call is almost always the program being
/// launched (e.g. `subprocess.run(["ffmpeg", ...])`).
static CONTEXT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?:subprocess\.(?:run|Popen|call|check_output|check_call)|Popen|(?:ctypes\.)?CDLL|execSync|execFileSync|execFile|spawn|exec|system|popen)\(\s*(?:\[)?\s*["']([A-Za-z0-9_./-]+)["']"#,
    )
    .expect("valid context regex")
});

/// Matches any known binary name as a whole word, case-insensitively.
static KNOWN_RE: LazyLock<Regex> = LazyLock::new(|| {
    let pattern = KNOWN_BINARIES
        .iter()
        .map(|b| regex::escape(b))
        .collect::<Vec<_>>()
        .join("|");
    Regex::new(&format!(r"(?i)\b(?:{pattern})\b")).expect("valid known-binary regex")
});

/// Executables referenced by an app that are not visible to static manifests.
pub struct HiddenDeps {
    /// Deduplicated, sorted executable names (lowercased).
    pub executables: Vec<String>,
}

/// Scan `app_dir` for hidden runtime dependencies and return them.
///
/// `_runtime` is reserved for future runtime-aware heuristics; the current
/// scan is runtime-agnostic. The walk is bounded (depth, file count, total
/// bytes) so a vendored or huge tree cannot hang the build.
pub fn scan_hidden_deps(app_dir: &Path, _runtime: Runtime) -> HiddenDeps {
    let mut found = BTreeSet::new();
    let mut files_scanned = 0usize;
    let mut bytes_scanned: u64 = 0;
    let mut stack = vec![(app_dir.to_path_buf(), 0u32)];

    while let Some((dir, depth)) = stack.pop() {
        if depth >= MAX_DEPTH {
            continue;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let meta = match fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if SKIP_DIRS.contains(&name.as_ref()) {
                    continue;
                }
                stack.push((path, depth + 1));
            } else if supported_ext(&path) && files_scanned < MAX_FILES && bytes_scanned < MAX_BYTES
            {
                if let Ok(content) = fs::read_to_string(&path) {
                    files_scanned += 1;
                    bytes_scanned += content.len() as u64;
                    for line in content.lines() {
                        for name in extract_names_from_line(line) {
                            found.insert(name);
                        }
                    }
                }
            }
        }
        if files_scanned >= MAX_FILES || bytes_scanned >= MAX_BYTES {
            break;
        }
    }

    let mut executables: Vec<String> = found.into_iter().collect();
    executables.sort();
    HiddenDeps { executables }
}

/// Extract candidate executable names from a single source line.
///
/// Combines two heuristics: the first quoted argument of a subprocess/exec/
/// dlopen call, and any known binary name appearing anywhere in the line (as a
/// whole word). Returns a deduplicated, sorted vector.
pub fn extract_names_from_line(line: &str) -> Vec<String> {
    let mut names = BTreeSet::new();
    for cap in CONTEXT_RE.captures_iter(line) {
        if let Some(m) = cap.get(1) {
            names.insert(clean_exec_name(m.as_str()).to_string());
        }
    }
    for m in KNOWN_RE.find_iter(line) {
        names.insert(m.as_str().to_lowercase());
    }
    names.into_iter().collect()
}

/// Whether `path` has a source extension worth scanning.
fn supported_ext(path: &Path) -> bool {
    let Some(ext) = path.extension() else {
        return false;
    };
    let ext = ext.to_string_lossy();
    SOURCE_EXTS.contains(&ext.as_ref())
}

/// Reduce an executable token to its bare name, dropping any directory prefix
/// (e.g. `/usr/bin/ffmpeg` -> `ffmpeg`). Extensions like `.so` are kept.
fn clean_exec_name(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name).trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn scan_detects_subprocess_executables() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "app.py",
            "subprocess.run([\"ffmpeg\", \"-i\", \"in.mp4\"])\n",
        );
        let deps = scan_hidden_deps(tmp.path(), Runtime::Python);
        assert!(deps.executables.iter().any(|e| e == "ffmpeg"));
    }

    #[test]
    fn scan_detects_ctypes_so() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "a.py",
            "import ctypes\nctypes.CDLL(\"libfoo.so\")\n",
        );
        let deps = scan_hidden_deps(tmp.path(), Runtime::Python);
        assert!(deps
            .executables
            .iter()
            .any(|e| e == "libfoo.so" || e == "libfoo"));
    }

    #[test]
    fn scan_detects_node_child_process() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "app.js",
            "require('child_process').execSync('curl https://example.com')\n",
        );
        let deps = scan_hidden_deps(tmp.path(), Runtime::Node);
        assert!(deps.executables.iter().any(|e| e == "curl"));
    }

    #[test]
    fn scan_ignores_comments_and_strings_when_reasonable() {
        // A commented-out call must not crash; a live generic binary in a
        // shell script should still be reported.
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "run.sh",
            "#!/bin/sh\n# git clone...\nffmpeg -i in.mp4\n",
        );
        let deps = scan_hidden_deps(tmp.path(), Runtime::Binary);
        assert!(!deps.executables.is_empty());
        assert!(deps.executables.iter().any(|e| e == "ffmpeg"));
    }

    #[test]
    fn scan_skips_node_modules() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "node_modules/pkg/index.js",
            "child_process.exec('curl http://example.com')\n",
        );
        write(tmp.path(), "main.js", "child_process.exec('git status')\n");
        let deps = scan_hidden_deps(tmp.path(), Runtime::Node);
        assert!(!deps.executables.iter().any(|e| e == "curl"));
        assert!(deps.executables.iter().any(|e| e == "git"));
    }
}
