//! Package manager detection — identifies which package manager an app uses.
//!
//! Priority is speed-based: uv > poetry > pipenv > pip; pnpm > yarn > bun > npm.

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
        }
    }

    pub fn install_cmd(&self) -> Vec<&'static str> {
        match self {
            Self::Uv => vec!["uv", "sync"],
            Self::Poetry => vec!["poetry", "install", "--no-interaction"],
            Self::Pipenv => vec!["pipenv", "install", "--deploy"],
            Self::Pip => vec!["pip", "install", "-r", "requirements.txt"],
            Self::Pnpm => vec!["pnpm", "install", "--frozen-lockfile"],
            Self::Yarn => vec!["yarn", "install", "--frozen-lockfile"],
            Self::Bun => vec!["bun", "install", "--frozen-lockfile"],
            Self::Npm => vec!["npm", "ci"],
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

/// Detect the package manager for a given runtime.
pub fn detect_pkgmgr(dir: &Path, runtime: &str) -> Option<PkgMgr> {
    match runtime {
        "python" => detect_python_pkgmgr(dir),
        "node" => detect_node_pkgmgr(dir),
        _ => None,
    }
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
}
