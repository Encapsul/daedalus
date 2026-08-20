//! Flatpak desktop packaging support.
//!
//! Generates `flatpak-manifest.json` + `.desktop` files from erebus build outputs,
//! enabling `flatpak-builder` to produce distributable Flatpak packages.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Packaging format selected via `--package-format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageFormat {
    #[default]
    Erebus,
    Flatpak,
    Snap,
}

impl fmt::Display for PackageFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Erebus => write!(f, "erebus"),
            Self::Flatpak => write!(f, "flatpak"),
            Self::Snap => write!(f, "snap"),
        }
    }
}

impl FromStr for PackageFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "erebus" => Ok(Self::Erebus),
            "flatpak" => Ok(Self::Flatpak),
            "snap" => Ok(Self::Snap),
            other => Err(format!("unknown package format: {other}")),
        }
    }
}

/// Maps an erebus runtime tag to a Flatpak runtime + SDK.
///
/// Returns `Some((runtime, sdk))` or `None` if the runtime is unknown.
pub fn runtime_mapping(runtime: &str) -> Option<(&'static str, &'static str)> {
    match runtime {
        "python3" | "python" | "python3.11" | "python3.10" | "node" | "nodejs" | "node20"
        | "node18" | "ruby" => Some(("org.freedesktop.Platform", "org.freedesktop.Sdk")),
        "rust" | "cargo" | "go" => Some(("org.freedesktop.Sdk", "org.freedesktop.Sdk")),
        b if b.starts_with("binary:") => Some(("org.freedesktop.Sdk", "org.freedesktop.Sdk")),
        _ => None,
    }
}

/// Sanitize a name into a valid Flatpak application ID (reverse-DNS style).
pub fn sanitize_app_id(name: &str) -> String {
    if name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("app.{name}")
    } else {
        name.to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlatpakManifest {
    pub app_id: String,
    pub runtime: String,
    pub runtime_version: String,
    pub sdk: String,
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub finish_args: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<FlatpakModule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlatpakModule {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buildsystem: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<FlatpakSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum FlatpakSource {
    File {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dest: Option<String>,
    },
    Dir {
        path: String,
    },
}

/// Generate a Flatpak manifest from erebus metadata.
pub fn generate_manifest(
    app_name: &str,
    runtime: &str,
    entrypoint: &[String],
    layers: &[crate::layer::SerializableLayer],
) -> std::io::Result<FlatpakManifest> {
    let (flatpak_runtime, flatpak_sdk) =
        runtime_mapping(runtime).unwrap_or(("org.freedesktop.Platform", "org.freedesktop.Sdk"));

    let command = entrypoint
        .first()
        .cloned()
        .unwrap_or_else(|| runtime.to_string());

    let app_id = sanitize_app_id(app_name);

    let mut modules = Vec::new();

    // Main app module from erebus payload (the .ere file)
    modules.push(FlatpakModule {
        name: app_name.to_string(),
        buildsystem: Some("simple".to_string()),
        sources: vec![FlatpakSource::File {
            path: format!("{}.ere", app_name),
            dest: Some("rootfs".to_string()),
        }],
    });

    // Add each layer as a separate module
    for layer in layers {
        let layer_name = layer.name();
        modules.push(FlatpakModule {
            name: sanitize_app_id(layer_name).replace('.', "-"),
            buildsystem: Some("simple".to_string()),
            sources: vec![FlatpakSource::Dir {
                path: layer_name.to_string(),
            }],
        });
    }

    let finish_args = vec!["--share=network".to_string(), "--share=ipc".to_string()];

    Ok(FlatpakManifest {
        app_id,
        runtime: flatpak_runtime.to_string(),
        runtime_version: "22.08".to_string(),
        sdk: flatpak_sdk.to_string(),
        command,
        finish_args,
        modules,
    })
}

/// Generate a `.desktop` file for GNOME/KDE menus.
pub fn generate_desktop_file(app_id: &str, app_name: &str) -> String {
    format!(
        r"[Desktop Entry]
Type=Application
Name={app_name}
Exec={app_id}
Terminal=false
Categories=Application;
Icon=application-default-icon
",
        app_name = app_name,
        app_id = app_id
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_format_parse() {
        assert_eq!(PackageFormat::default(), PackageFormat::Erebus);
        assert_eq!(
            "flatpak".parse::<PackageFormat>().unwrap(),
            PackageFormat::Flatpak
        );
        assert!(".snap".parse::<PackageFormat>().is_err());
    }

    #[test]
    fn flatpak_runtime_mapping() {
        assert_eq!(
            runtime_mapping("python3"),
            Some(("org.freedesktop.Platform", "org.freedesktop.Sdk"))
        );
        assert_eq!(
            runtime_mapping("node"),
            Some(("org.freedesktop.Platform", "org.freedesktop.Sdk"))
        );
        assert_eq!(
            runtime_mapping("binary:/app/app"),
            Some(("org.freedesktop.Sdk", "org.freedesktop.Sdk"))
        );
        assert!(runtime_mapping("unknown-runtime").is_none());
    }

    #[test]
    fn sanitize_app_id_tests() {
        assert_eq!(sanitize_app_id("my-app"), "my-app");
        assert_eq!(sanitize_app_id("my app"), "my app");
        assert_eq!(sanitize_app_id("123app"), "app.123app");
    }

    #[test]
    fn manifest_generation() {
        let manifest = generate_manifest(
            "myapp",
            "python3",
            &["python3".to_string(), "/app/main.py".to_string()],
            &[],
        )
        .unwrap();

        assert_eq!(manifest.app_id, "myapp");
        assert_eq!(manifest.runtime, "org.freedesktop.Platform");
        assert_eq!(manifest.sdk, "org.freedesktop.Sdk");
        assert_eq!(manifest.command, "python3");
        assert!(!manifest.modules.is_empty());
    }

    #[test]
    fn desktop_file_generation() {
        let desktop = generate_desktop_file("myapp", "My App");
        assert!(desktop.contains("Name=My App"));
        assert!(desktop.contains("Exec=myapp"));
        assert!(desktop.contains("[Desktop Entry]"));
    }
}
