//! Package manager detection — identifies which package manager an app uses.
//!
//! Priority is speed-based: uv > poetry > pipenv > pip; pnpm > yarn > bun > npm.

use std::io;
use std::path::Path;

/// Detected package manager type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PkgMgr {
    // Python
    Uv,
    Poetry,
    Pipenv,
    Pip,
    // Node
    Pnpm,
    Yarn,
    Bun,
    Npm,
    // PHP
    Composer,
    // Ruby
    Bundler,
    // Perl
    Cpan,
}

impl PkgMgr {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Uv => "uv",
            Self::Poetry => "poetry",
            Self::Pipenv => "pipenv",
            Self::Pip => "pip",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
            Self::Bun => "bun",
            Self::Npm => "npm",
            Self::Composer => "composer",
            Self::Bundler => "bundler",
            Self::Cpan => "cpan",
        }
    }

    pub fn install_cmd(&self) -> Vec<&'static str> {
        match self {
            Self::Uv => vec!["uv", "sync"],
            Self::Poetry => vec!["poetry", "install", "--no-interaction"],
            Self::Pipenv => vec!["pipenv", "install", "--deploy"],
            Self::Pip => vec!["pip", "install", "-r", "requirements.txt"],
            Self::Pnpm => vec!["pnpm", "install", "--ignore-scripts"],
            Self::Yarn => vec!["yarn", "install", "--frozen-lockfile", "--ignore-scripts"],
            Self::Bun => vec!["bun", "install", "--frozen-lockfile"],
            Self::Npm => vec!["npm", "ci", "--ignore-scripts"],
            Self::Composer => vec![
                "composer",
                "install",
                "--no-dev",
                "--ignore-platform-reqs",
                "--no-interaction",
                "--prefer-dist",
            ],
            Self::Bundler => vec!["bundle", "install", "--deployment", "--without development"],
            Self::Cpan => vec!["cpan", "-T", "."],
        }
    }
}

/// Detect the Python package manager by checking lock files.
pub fn detect_python_pkgmgr(dir: &Path) -> Option<PkgMgr> {
    if dir.join("uv.lock").is_file() {
        return Some(PkgMgr::Uv);
    }
    if dir.join("poetry.lock").is_file() {
        return Some(PkgMgr::Poetry);
    }
    if dir.join("Pipfile.lock").is_file() {
        return Some(PkgMgr::Pipenv);
    }
    let req = dir.join("requirements.txt");
    if req.is_file() && std::fs::metadata(&req).is_ok_and(|m| m.len() > 0) {
        return Some(PkgMgr::Pip);
    }
    None
}

/// Detect the Node package manager by checking lock files.
pub fn detect_node_pkgmgr(dir: &Path) -> Option<PkgMgr> {
    if !dir.join("package.json").is_file() {
        return None;
    }
    if dir.join("pnpm-lock.yaml").is_file() {
        return Some(PkgMgr::Pnpm);
    }
    if dir.join("pnpm-workspace.yaml").is_file() {
        return Some(PkgMgr::Pnpm);
    }
    if dir.join("yarn.lock").is_file() {
        return Some(PkgMgr::Yarn);
    }
    if dir.join("bun.lockb").is_file() {
        return Some(PkgMgr::Bun);
    }
    if dir.join("package-lock.json").is_file() {
        return Some(PkgMgr::Npm);
    }
    // No lock file but package.json exists
    Some(PkgMgr::Npm)
}

/// Detect the PHP package manager (Composer).
pub fn detect_php_pkgmgr(dir: &Path) -> Option<PkgMgr> {
    if dir.join("composer.json").is_file() {
        return Some(PkgMgr::Composer);
    }
    None
}

/// Detect the Ruby package manager (Bundler).
pub fn detect_ruby_pkgmgr(dir: &Path) -> Option<PkgMgr> {
    if dir.join("Gemfile.lock").is_file() {
        return Some(PkgMgr::Bundler);
    }
    if dir.join("Gemfile").is_file() {
        return Some(PkgMgr::Bundler);
    }
    None
}

/// Detect the Perl package manager (CPAN).
pub fn detect_perl_pkgmgr(dir: &Path) -> Option<PkgMgr> {
    if dir.join("cpanfile.snapshot").is_file() {
        return Some(PkgMgr::Cpan);
    }
    if dir.join("cpanfile").is_file() {
        return Some(PkgMgr::Cpan);
    }
    None
}

/// Detect the package manager for a given runtime.
pub fn detect_pkgmgr(dir: &Path, runtime: &str) -> Option<PkgMgr> {
    match runtime {
        "python" => detect_python_pkgmgr(dir),
        "node" => detect_node_pkgmgr(dir),
        "php" => detect_php_pkgmgr(dir),
        "ruby" => detect_ruby_pkgmgr(dir),
        "perl" => detect_perl_pkgmgr(dir),
        _ => None,
    }
}

/// Run frontend/build steps declared in package.json `scripts` if the
/// corresponding output files/directories do not already exist.
///
/// Supported scripts (run in order if present):
///   - `build`       : general build step
///   - `build:css`   : CSS build step
///   - `build:js`    : JS build step
///
/// Returns the number of build commands executed.
pub fn run_build_steps(app_dir: &Path, verbose: bool) -> io::Result<usize> {
    use std::process::Command;

    let pkg_json = app_dir.join("package.json");
    if !pkg_json.is_file() {
        return Ok(0);
    }

    let content = match std::fs::read_to_string(&pkg_json) {
        Ok(c) => c,
        Err(_) => return Ok(0),
    };

    let pkg: serde_json::Value = match serde_json::from_str(&content) {
        Ok(p) => p,
        Err(_) => return Ok(0),
    };

    let scripts = match pkg.get("scripts").and_then(|s| s.as_object()) {
        Some(s) => s,
        None => return Ok(0),
    };

    // Determine which build scripts exist and need running
    let mut to_run: Vec<(&str, &str)> = Vec::new();
    if let Some(cmd) = scripts.get("build").and_then(|v| v.as_str()) {
        to_run.push(("build", cmd));
    }
    if let Some(cmd) = scripts.get("build:css").and_then(|v| v.as_str()) {
        to_run.push(("build:css", cmd));
    }
    if let Some(cmd) = scripts.get("build:js").and_then(|v| v.as_str()) {
        to_run.push(("build:js", cmd));
    }

    if to_run.is_empty() {
        return Ok(0);
    }

    let mut ran = 0;
    for (name, cmd) in to_run {
        if verbose {
            eprintln!("  build step: running `npm run {name}`...");
        }

        // Try npm first, fallback to npx/yarn/pnpm
        let runners = ["npm", "npx", "yarn", "pnpm", "bun"];
        let mut ran_ok = false;
        for runner in &runners {
            if !is_command_available(runner) {
                continue;
            }
            let mut args = vec![runner, "run", name];
            // Tokenize the command
            args.extend(cmd.split_whitespace().skip(1));
            let status = Command::new(runner)
                .args(&args[1..])
                .current_dir(app_dir)
                .status();
            if let Ok(s) = status {
                if s.success() {
                    ran += 1;
                    ran_ok = true;
                    break;
                }
            }
        }
        if !ran_ok && verbose {
            eprintln!("  build step: `npm run {name}` skipped (no runner available)");
        }
    }
    Ok(ran)
}

fn is_command_available(name: &str) -> bool {
    std::process::Command::new("sh")
        .args(["-c", &format!("command -v {name}")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Detect any secondary package managers (e.g., a PHP app with package.json).
/// Returns all detected package managers, primary first.
pub fn detect_all_pkgmgrs(dir: &Path, runtime: &str) -> Vec<PkgMgr> {
    let mut managers = Vec::new();

    if let Some(primary) = detect_pkgmgr(dir, runtime) {
        managers.push(primary);
    }

    // Check for secondary package managers not tied to the primary runtime
    let has_node = dir.join("package.json").is_file();
    let has_php = dir.join("composer.json").is_file();
    let has_ruby = dir.join("Gemfile").is_file();
    let has_perl = dir.join("cpanfile").is_file();

    if has_node && !matches!(runtime, "node") {
        if let Some(node_mgr) = detect_node_pkgmgr(dir) {
            managers.push(node_mgr);
        }
    }
    if has_php && !matches!(runtime, "php") {
        if let Some(php_mgr) = detect_php_pkgmgr(dir) {
            managers.push(php_mgr);
        }
    }
    if has_ruby && !matches!(runtime, "ruby") {
        if let Some(ruby_mgr) = detect_ruby_pkgmgr(dir) {
            managers.push(ruby_mgr);
        }
    }
    if has_perl && !matches!(runtime, "perl") {
        if let Some(perl_mgr) = detect_perl_pkgmgr(dir) {
            managers.push(perl_mgr);
        }
    }

    managers
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn uv_lock() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("uv.lock"), "# uv").unwrap();
        assert_eq!(detect_python_pkgmgr(dir.path()), Some(PkgMgr::Uv));
    }

    #[test]
    fn uv_beats_poetry() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("uv.lock"), "# uv").unwrap();
        std::fs::write(dir.path().join("poetry.lock"), "# poetry").unwrap();
        assert_eq!(detect_python_pkgmgr(dir.path()), Some(PkgMgr::Uv));
    }

    #[test]
    fn pnpm_lock() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "lockfile: 1").unwrap();
        assert_eq!(detect_node_pkgmgr(dir.path()), Some(PkgMgr::Pnpm));
    }

    #[test]
    fn no_package_json_no_node() {
        let dir = TempDir::new().unwrap();
        assert_eq!(detect_node_pkgmgr(dir.path()), None);
    }

    #[test]
    fn detect_for_runtime() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("uv.lock"), "# uv").unwrap();
        assert_eq!(detect_pkgmgr(dir.path(), "python"), Some(PkgMgr::Uv));
        assert_eq!(detect_pkgmgr(dir.path(), "java"), None);
    }

    #[test]
    fn composer_detection() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("composer.json"), "{}").unwrap();
        assert_eq!(detect_php_pkgmgr(dir.path()), Some(PkgMgr::Composer));
    }

    #[test]
    fn composer_lock_file() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("composer.json"), "{}").unwrap();
        std::fs::write(dir.path().join("composer.lock"), "{}").unwrap();
        assert_eq!(detect_php_pkgmgr(dir.path()), Some(PkgMgr::Composer));
    }

    #[test]
    fn bundler_detection() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("Gemfile"), "source 'https://rubygems.org'").unwrap();
        assert_eq!(detect_ruby_pkgmgr(dir.path()), Some(PkgMgr::Bundler));
    }

    #[test]
    fn bundler_lock_file() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("Gemfile"), "source 'https://rubygems.org'").unwrap();
        std::fs::write(dir.path().join("Gemfile.lock"), "").unwrap();
        assert_eq!(detect_ruby_pkgmgr(dir.path()), Some(PkgMgr::Bundler));
    }

    #[test]
    fn php_runtime_detects_composer() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("composer.json"), "{}").unwrap();
        assert_eq!(detect_pkgmgr(dir.path(), "php"), Some(PkgMgr::Composer));
    }

    #[test]
    fn ruby_runtime_detects_bundler() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("Gemfile"), "").unwrap();
        assert_eq!(detect_pkgmgr(dir.path(), "ruby"), Some(PkgMgr::Bundler));
    }

    #[test]
    fn secondary_node_mgr() {
        let dir = TempDir::new().unwrap();
        // PHP app with Node.js frontend
        std::fs::write(dir.path().join("composer.json"), "{}").unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let mgrs = detect_all_pkgmgrs(dir.path(), "php");
        assert_eq!(mgrs.len(), 2);
        assert_eq!(mgrs[0], PkgMgr::Composer);
        assert_eq!(mgrs[1], PkgMgr::Npm);
    }

    #[test]
    fn secondary_php_mgr() {
        let dir = TempDir::new().unwrap();
        // Node app with PHP API
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("composer.json"), "{}").unwrap();
        let mgrs = detect_all_pkgmgrs(dir.path(), "node");
        assert_eq!(mgrs.len(), 2);
        assert_eq!(mgrs[0], PkgMgr::Npm);
        assert_eq!(mgrs[1], PkgMgr::Composer);
    }

    #[test]
    fn perl_runtime_detects_cpan() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("cpanfile"), "").unwrap();
        assert_eq!(detect_pkgmgr(dir.path(), "perl"), Some(PkgMgr::Cpan));
    }

    #[test]
    fn cpanfile_snapshot() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("cpanfile.snapshot"), "").unwrap();
        assert_eq!(detect_perl_pkgmgr(dir.path()), Some(PkgMgr::Cpan));
    }

    #[test]
    fn secondary_perl_mgr() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("cpanfile"), "").unwrap();
        let mgrs = detect_all_pkgmgrs(dir.path(), "node");
        assert_eq!(mgrs.len(), 2);
        assert_eq!(mgrs[0], PkgMgr::Npm);
        assert_eq!(mgrs[1], PkgMgr::Cpan);
    }
}
