use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

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

const JS_EXTS: &[&str] = &["js", "mjs", "cjs", "ts", "jsx", "tsx"];
const CSS_EXTS: &[&str] = &["css"];

fn is_skip_dir(name: &str) -> bool {
    SKIP_DIRS.contains(&name)
}

fn has_terser() -> bool {
    Command::new("which")
        .arg("terser")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn minify_js_file(path: &Path) -> bool {
    if !has_terser() {
        return false;
    }
    Command::new("terser")
        .arg(path)
        .arg("--compress")
        .arg("--mangle")
        .arg("-o")
        .arg(path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn minify_css(content: &str) -> String {
    let comments = Regex::new(r"(?s)/\*.*?\*/").unwrap();
    let ws = Regex::new(r"\s+").unwrap();
    let sel_open = Regex::new(r"\s*\{\s*").unwrap();
    let sel_close = Regex::new(r"\s*\}\s*").unwrap();
    let colon = Regex::new(r"\s*:\s*").unwrap();
    let semi = Regex::new(r"\s*;\s*").unwrap();

    let result = comments.replace_all(content, "");
    let result = ws.replace_all(&result, " ");
    let result = sel_open.replace_all(&result, "{");
    let result = sel_close.replace_all(&result, "}");
    let result = colon.replace_all(&result, ":");
    let result = semi.replace_all(&result, ";");
    result.trim().to_string()
}

pub fn minify_app_dir(app_dir: &Path, verbose: bool) -> io::Result<usize> {
    let mut minified = 0;

    let walker = fs::read_dir(app_dir)?;
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

        let ext = match path.extension().and_then(|e| e.to_str()) {
            Some(e) => e,
            None => continue,
        };

        if JS_EXTS.contains(&ext) {
            if minify_js_file(&path) {
                minified += 1;
                if verbose {
                    eprintln!("  minify: {} (JS/TS)", path.display());
                }
            }
        } else if CSS_EXTS.contains(&ext) {
            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let minified_content = minify_css(&content);
            if minified_content.len() < content.len() {
                fs::write(&path, &minified_content)?;
                minified += 1;
                if verbose {
                    eprintln!("  minify: {} (CSS)", path.display());
                }
            }
        }
    }

    Ok(minified)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minify_css_strips_comments() {
        let input = "/* comment */ body { color: red; }";
        let result = minify_css(input);
        assert_eq!(result, "body{color:red;}");
    }

    #[test]
    fn test_minify_css_collapse_whitespace() {
        let input = "  body   {   color  :  red  ;  }  ";
        let result = minify_css(input);
        assert_eq!(result, "body{color:red;}");
    }

    #[test]
    fn test_minify_css_realistic() {
        let input = r"
            /* Main stylesheet */
            .container {
                max-width: 1200px;
                margin: 0 auto;
            }

            .header {
                background-color: #fff;
                padding: 1rem;
            }
        ";
        let result = minify_css(input);
        assert!(result.contains(".container{"));
        assert!(!result.contains("/*"));
        assert!(!result.contains('\n'));
    }
}
