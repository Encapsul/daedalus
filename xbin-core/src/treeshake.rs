use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;

use regex::Regex;

const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "__pycache__",
    ".venv",
    "venv",
    ".xbin",
    "dist",
    "build",
    ".next",
    ".nuxt",
    ".output",
    "coverage",
];

const JS_EXTS: &[&str] = &["js", "ts", "jsx", "tsx", "mjs", "cjs"];

fn req_re() -> Regex {
    Regex::new(r#"require\s*\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap()
}

fn import_re() -> Regex {
    Regex::new(r#"import\s+(?:.*?\s+from\s+)?['"]([^'"]+)['"]|import\s*\(\s*['"]([^'"]+)['"]\s*\)"#)
        .unwrap()
}

fn is_skip_dir(name: &str) -> bool {
    SKIP_DIRS.contains(&name)
}

fn is_js_ext(ext: &str) -> bool {
    JS_EXTS.contains(&ext)
}

pub fn is_package_spec(spec: &str) -> bool {
    if spec.starts_with('.') || spec.starts_with('/') {
        return false;
    }
    if spec.starts_with('@') {
        return spec.split('/').count() >= 2;
    }
    true
}

pub fn extract_package_name(spec: &str) -> String {
    if spec.starts_with('@') {
        let parts: Vec<&str> = spec.split('/').collect();
        if parts.len() >= 2 {
            return format!("{}/{}", parts[0], parts[1]);
        }
        return spec.to_string();
    }
    spec.split('/').next().unwrap_or(spec).to_string()
}

fn parse_package_json(app_dir: &Path) -> HashMap<String, String> {
    let pkg = app_dir.join("package.json");
    if !pkg.is_file() {
        return HashMap::new();
    }
    let content = match fs::read_to_string(&pkg) {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };
    let data: serde_json::Value = match serde_json::from_str(&content) {
        Ok(d) => d,
        Err(_) => return HashMap::new(),
    };
    let mut deps = HashMap::new();
    if let Some(obj) = data.get("dependencies").and_then(|v| v.as_object()) {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                deps.insert(k.clone(), s.to_string());
            }
        }
    }
    if let Some(obj) = data.get("devDependencies").and_then(|v| v.as_object()) {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                deps.insert(k.clone(), s.to_string());
            }
        }
    }
    deps
}

fn scan_imports_in_file(path: &Path) -> HashSet<String> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return HashSet::new(),
    };
    let req = req_re();
    let imp = import_re();
    let mut found = HashSet::new();
    for cap in req.captures_iter(&content) {
        if let Some(m) = cap.get(1) {
            found.insert(m.as_str().to_string());
        }
    }
    for cap in imp.captures_iter(&content) {
        let spec = cap.get(1).or_else(|| cap.get(2));
        if let Some(s) = spec {
            found.insert(s.as_str().to_string());
        }
    }
    found
}

pub fn detect_used_packages(app_dir: &Path) -> HashSet<String> {
    let pkg_deps = parse_package_json(app_dir);
    if pkg_deps.is_empty() {
        return HashSet::new();
    }

    let mut all_imports: HashSet<String> = HashSet::new();

    let walker = match fs::read_dir(app_dir) {
        Ok(w) => w,
        Err(_) => return HashSet::new(),
    };

    let mut stack: Vec<_> = walker.filter_map(Result::ok).collect();

    while let Some(entry) = stack.pop() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if is_skip_dir(name) {
                    continue;
                }
            }
            if let Ok(read) = fs::read_dir(&path) {
                for e in read.filter_map(Result::ok) {
                    stack.push(e);
                }
            }
            continue;
        }
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if is_js_ext(ext) {
                eprintln!("scanning: {}", path.display());
                all_imports.extend(scan_imports_in_file(&path));
            }
        }
    }

    let mut used = HashSet::new();
    for spec in &all_imports {
        if !is_package_spec(spec) {
            continue;
        }
        let pkg_name = extract_package_name(spec);
        if pkg_deps.contains_key(&pkg_name) {
            used.insert(pkg_name);
        }
    }
    used
}

pub fn prune_node_modules(app_dir: &Path, verbose: bool) -> io::Result<usize> {
    let nm = app_dir.join("node_modules");
    if !nm.is_dir() {
        return Ok(0);
    }

    let used = detect_used_packages(app_dir);
    if used.is_empty() {
        return Ok(0);
    }

    let mut removed = 0;
    let entries: Vec<_> = fs::read_dir(&nm)?.filter_map(Result::ok).collect();

    for entry in &entries {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        if name.starts_with('@') {
            if let Ok(sub_entries) = fs::read_dir(&path) {
                for sub in sub_entries.filter_map(Result::ok) {
                    if !sub.path().is_dir() {
                        continue;
                    }
                    let sub_name = match sub.file_name().to_str() {
                        Some(n) => n.to_string(),
                        None => continue,
                    };
                    let pkg_name = format!("{name}/{sub_name}");
                    if !used.contains(&pkg_name) {
                        fs::remove_dir_all(sub.path())?;
                        removed += 1;
                        if verbose {
                            eprintln!("  tree-shake: removed {pkg_name}");
                        }
                    }
                }
            }
        } else if !used.contains(&name) {
            fs::remove_dir_all(&path)?;
            removed += 1;
            if verbose {
                eprintln!("  tree-shake: removed {name}");
            }
        }
    }

    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_is_package_spec() {
        assert!(is_package_spec("lodash"));
        assert!(is_package_spec("lodash/fp"));
        assert!(is_package_spec("@scope/name"));
        assert!(is_package_spec("@scope/name/sub"));
        assert!(!is_package_spec("./foo"));
        assert!(!is_package_spec("../bar"));
        assert!(!is_package_spec("/absolute"));
    }

    #[test]
    fn test_extract_package_name_scoped() {
        assert_eq!(extract_package_name("@scope/name"), "@scope/name");
        assert_eq!(extract_package_name("@scope/name/sub"), "@scope/name");
        assert_eq!(extract_package_name("lodash/fp"), "lodash");
        assert_eq!(extract_package_name("react"), "react");
    }

    #[test]
    fn test_scan_imports_require() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("app.js");
        fs::write(
            &file,
            r"const foo = require('lodash');
const bar = require('react');
const local = require('./helper');
",
        )
        .unwrap();

        let imports = scan_imports_in_file(&file);
        assert!(imports.contains("lodash"));
        assert!(imports.contains("react"));
        assert!(imports.contains("./helper"));
        assert_eq!(imports.len(), 3);
    }

    #[test]
    fn test_scan_imports_esm() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("app.mjs");
        fs::write(
            &file,
            r"import React from 'react';
import { join } from 'path';
import('fs');
import lodash from '@scope/pkg';
",
        )
        .unwrap();

        let imports = scan_imports_in_file(&file);
        assert!(imports.contains("react"));
        assert!(imports.contains("path"));
        assert!(imports.contains("fs"));
        assert!(imports.contains("@scope/pkg"));
    }

    #[test]
    fn test_detect_package_json_deps() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies":{"react":"^18"},"devDependencies":{"jest":"^29"}}"#,
        )
        .unwrap();
        let file = dir.path().join("index.js");
        fs::write(&file, "const React = require('react');").unwrap();

        let all_imports = scan_imports_in_file(&file);
        eprintln!("all_imports: {all_imports:?}");
        let used = detect_used_packages(dir.path());
        eprintln!("used: {used:?}");
        assert!(used.contains("react"));
        assert!(!used.contains("jest"));
    }

    #[test]
    fn test_prune_removes_unused() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies":{"react":"^18","lodash":"^4"}}"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("index.js"),
            "const React = require('react');",
        )
        .unwrap();

        fs::create_dir_all(dir.path().join("node_modules/react")).unwrap();
        fs::create_dir_all(dir.path().join("node_modules/lodash")).unwrap();
        fs::write(dir.path().join("node_modules/react/index.js"), "").unwrap();
        fs::write(dir.path().join("node_modules/lodash/index.js"), "").unwrap();

        let removed = prune_node_modules(dir.path(), false).unwrap();
        assert_eq!(removed, 1);
        assert!(!dir.path().join("node_modules/lodash").is_dir());
        assert!(dir.path().join("node_modules/react").is_dir());
    }

    #[test]
    fn test_prune_keeps_used() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies":{"react":"^18"}}"#,
        )
        .unwrap();
        fs::write(dir.path().join("index.js"), "import React from 'react';").unwrap();

        fs::create_dir_all(dir.path().join("node_modules/react")).unwrap();
        fs::write(dir.path().join("node_modules/react/index.js"), "").unwrap();

        let removed = prune_node_modules(dir.path(), false).unwrap();
        assert_eq!(removed, 0);
        assert!(dir.path().join("node_modules/react").is_dir());
    }
}
