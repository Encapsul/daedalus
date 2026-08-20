//! Flatpak desktop packaging support.
//!
//! Generates `flatpak-manifest.json` + `.desktop` files from erebus build outputs,
//! enabling `flatpak-builder` to produce distributable Flatpak packages.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Desktop environment target for runtime-specific packaging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DesktopEnv {
    #[default]
    Unknown,
    Gnome,
    KDE,
    XFCE,
    Any,
}

impl DesktopEnv {
    pub fn from_env() -> Self {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP")
            .or_else(|_| std::env::var("XDG_SESSION_DESKTOP"))
            .unwrap_or_default()
            .to_lowercase();

        if desktop.contains("gnome") {
            Self::Gnome
        } else if desktop.contains("kde") {
            Self::KDE
        } else if desktop.contains("xfce") {
            Self::XFCE
        } else if desktop.contains("unity") || desktop.contains("ubuntu") {
            Self::Any
        } else {
            Self::Unknown
        }
    }
}

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
///
/// For desktop apps, returns the GNOME or KDE platform depending on `desktop_env`:
/// - `DesktopEnv::Gnome` → `org.gnome.Platform`
/// - `DesktopEnv::KDE` → `org.kde.Platform`
/// - `DesktopEnv::Any`/`Unknown` → `org.freedesktop.Platform` (universal fallback)
pub fn runtime_mapping(
    runtime: &str,
    desktop_env: DesktopEnv,
) -> Option<(&'static str, &'static str)> {
    let (platform_runtime, _) = platform_for_desktop(desktop_env);
    let sdk_runtime = "org.freedesktop.Sdk";
    let sdk_sdk = "org.freedesktop.Sdk";

    match runtime {
        "python3" | "python" | "python3.11" | "python3.10" | "node" | "nodejs" | "node20"
        | "node18" | "ruby" => Some((platform_runtime, sdk_sdk)),
        "rust" | "cargo" | "go" => Some((sdk_runtime, sdk_sdk)),
        b if b.starts_with("binary:") => Some((sdk_runtime, sdk_sdk)),
        _ => None,
    }
}

/// Flatpak runtime + SDK for a given desktop environment.
pub fn platform_for_desktop(env: DesktopEnv) -> (&'static str, &'static str) {
    match env {
        DesktopEnv::Gnome => ("org.gnome.Platform", "org.gnome.Sdk"),
        DesktopEnv::KDE => ("org.kde.Platform", "org.kde.Sdk"),
        DesktopEnv::XFCE => ("org.xfce.Panel-4.18", "org.xfce.Sdk"),
        DesktopEnv::Any | DesktopEnv::Unknown => {
            ("org.freedesktop.Platform", "org.freedesktop.Sdk")
        }
    }
}

/// Finish args for desktop GUI apps (X11/Wayland/GL).
pub fn desktop_finish_args() -> Vec<String> {
    vec![
        "--share=ipc".to_string(),
        "--share=network".to_string(),
        "--socket=x11".to_string(),
        "--socket=wayland".to_string(),
        "--socket=opengl".to_string(),
        "--device=dri".to_string(),
        "--filesystem=host".to_string(),
    ]
}

/// Finish args for headless/service apps.
pub fn service_finish_args() -> Vec<String> {
    vec!["--share=network".to_string(), "--share=ipc".to_string()]
}

/// Sanitize a name into a valid Flatpak application ID (reverse-DNS style).
/// Flatpak requires at least 2 periods in the app ID.
pub fn sanitize_app_id(name: &str) -> String {
    let sanitized = name.replace([' ', '_', '-'], ".");
    if sanitized.matches('.').count() < 2 {
        format!("org.erebus.{sanitized}")
    } else {
        sanitized
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "build-commands"
    )]
    pub build_commands: Option<Vec<String>>,
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
///
/// When `is_desktop` is true, uses desktop-aware runtime + `finish_args`
/// (`finish_args` for X11/Wayland/GL). `desktop_env` selects the appropriate
/// platform (`GNOME`/`KDE`/`XFCE`/`Any`).
///
/// # Parameters
///
/// - `is_desktop`: whether to generate desktop/GUI-aware manifest (X11/Wayland)
/// - `desktop_env`: selects GNOME/KDE/XFCE/Any platform runtime
/// - `runtime`: erebus runtime tag (python3, node, go, binary:...)
/// - `entrypoint`: command vector (executable, args...)
/// - `layers`: additional erebus layers for modularity
pub fn generate_manifest(
    app_name: &str,
    runtime: &str,
    entrypoint: &[String],
    layers: &[crate::layer::SerializableLayer],
    is_desktop: bool,
    desktop_env: DesktopEnv,
) -> std::io::Result<FlatpakManifest> {
    let (flatpak_runtime, flatpak_sdk) = if is_desktop {
        runtime_mapping(runtime, desktop_env)
    } else {
        runtime_mapping(runtime, DesktopEnv::Unknown)
    }
    .unwrap_or(("org.freedesktop.Platform", "org.freedesktop.Sdk"));

    let command = entrypoint
        .first()
        .cloned()
        .unwrap_or_else(|| runtime.to_string());

    let app_id = sanitize_app_id(app_name);

    let mut modules = Vec::new();

    // Main app module from erebus payload (the .erebus file)
    modules.push(FlatpakModule {
        name: app_name.to_string(),
        buildsystem: Some("simple".to_string()),
        build_commands: Some(vec![format!(
            "install -D {app_name}.erebus ${{PREFIX}}/bin/{app_name}.erebus"
        )]),
        sources: vec![FlatpakSource::File {
            path: format!("{}.erebus", app_name),
            dest: Some(format!("rootfs/app/{app_name}.erebus")),
        }],
    });

    // Add each layer as a separate module
    for layer in layers {
        let layer_name = layer.name();
        modules.push(FlatpakModule {
            name: sanitize_app_id(layer_name).replace('.', "-"),
            buildsystem: Some("simple".to_string()),
            build_commands: None,
            sources: vec![FlatpakSource::Dir {
                path: layer_name.to_string(),
            }],
        });
    }

    let finish_args = if is_desktop {
        desktop_finish_args()
    } else {
        service_finish_args()
    };

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
            runtime_mapping("python3", DesktopEnv::Unknown),
            Some(("org.freedesktop.Platform", "org.freedesktop.Sdk"))
        );
        assert_eq!(
            runtime_mapping("python3", DesktopEnv::Gnome),
            Some(("org.gnome.Platform", "org.freedesktop.Sdk"))
        );
        assert_eq!(
            runtime_mapping("python3", DesktopEnv::KDE),
            Some(("org.kde.Platform", "org.freedesktop.Sdk"))
        );
        assert_eq!(
            runtime_mapping("node", DesktopEnv::Unknown),
            Some(("org.freedesktop.Platform", "org.freedesktop.Sdk"))
        );
        assert_eq!(
            runtime_mapping("binary:/app/app", DesktopEnv::Unknown),
            Some(("org.freedesktop.Sdk", "org.freedesktop.Sdk"))
        );
        assert!(runtime_mapping("unknown-runtime", DesktopEnv::Unknown).is_none());
    }

    #[test]
    fn sanitize_app_id_tests() {
        assert_eq!(sanitize_app_id("my-app"), "org.erebus.my.app");
        assert_eq!(sanitize_app_id("my.app.example"), "my.app.example");
        assert_eq!(sanitize_app_id("123app"), "org.erebus.123app");
    }

    #[test]
    fn manifest_generation() {
        let manifest = generate_manifest(
            "myapp",
            "python3",
            &["python3".to_string(), "/app/main.py".to_string()],
            &[],
            false,
            DesktopEnv::Unknown,
        )
        .unwrap();

        assert_eq!(manifest.app_id, "org.erebus.myapp");
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
