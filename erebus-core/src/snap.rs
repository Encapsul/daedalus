//! Snap packaging support.
//!
//! Generates `snapcraft.yaml` from erebus artifact metadata, enabling
//! `snapcraft` to produce distributable Snap packages.
//!
//! Desktop-aware: generates proper plugs, command-chain, and environment for
//! GUI applications (X11/Wayland/GL), based on patterns observed from real
//! snap packages (firefox, discord, vivaldi, code).

use std::collections::BTreeMap;
use std::io;

use serde::{Deserialize, Serialize};

/// A snapcraft.yaml configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapConfig {
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apps: Option<BTreeMap<String, SnapApp>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub services: Option<BTreeMap<String, SnapService>>,
    #[serde(rename = "parts", skip_serializing_if = "BTreeMap::is_empty")]
    pub parts: BTreeMap<String, SnapPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confinement: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", rename = "environment")]
    pub environment: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<SnapLayout>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapApp {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugs: Option<Vec<String>>,
    #[serde(rename = "command-chain", skip_serializing_if = "Option::is_none")]
    pub command_chain: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<BTreeMap<String, String>>,
    #[serde(rename = "common-id", skip_serializing_if = "Option::is_none")]
    pub common_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desktop: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapService {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugs: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SnapPart {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
    #[serde(rename = "source", skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(rename = "stage", skip_serializing_if = "Vec::is_empty")]
    pub stage: Vec<String>,
    #[serde(rename = "organize", skip_serializing_if = "BTreeMap::is_empty")]
    pub organize: BTreeMap<String, String>,
}

/// Layout directives for binding shared libraries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapLayoutEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symlink: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bind_file: Option<String>,
}

pub type SnapLayout = BTreeMap<String, SnapLayoutEntry>;

/// Desktop environment target for runtime-specific packaging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopEnv {
    Gnome,
    KDE,
    XFCE,
    Any,
    Unknown,
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
        } else if desktop.contains("ubuntu:unity") || desktop.contains("unity") {
            Self::Any
        } else {
            Self::Unknown
        }
    }
}

/// Plugs (interfaces) for desktop GUI apps.
pub fn desktop_plugs() -> Vec<&'static str> {
    vec![
        "desktop",
        "desktop-legacy",
        "gsettings",
        "opengl",
        "wayland",
        "x11",
        "audio-playback",
        "audio-record",
        "camera",
        "home",
        "network",
        "network-bind",
        "removable-media",
        "screen-inhibit-control",
        "shmem",
        "unity7",
        "upower-observe",
    ]
}

/// Non-GUI (service/cli) plugs.
pub fn cli_plugs() -> Vec<&'static str> {
    vec!["network", "home"]
}

/// Command-chain entry for desktop launch.
pub fn desktop_command_chain() -> Vec<String> {
    vec!["snap/command-chain/desktop-launch".to_string()]
}

/// Environment variables for desktop integration.
pub fn desktop_environment() -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert("GTK_USE_PORTAL".to_string(), "1".to_string());
    env.insert(
        "LD_LIBRARY_PATH".to_string(),
        "${SNAP_LIBRARY_PATH}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}:$SNAP/usr/lib:$SNAP/usr/lib/x86_64-linux-gnu".to_string(),
    );
    env.insert(
        "PATH".to_string(),
        "$SNAP/usr/sbin:$SNAP/usr/bin:$SNAP/sbin:$SNAP/bin:$PATH".to_string(),
    );
    env
}

/// Generate a snapcraft.yaml from artifact metadata.
pub fn generate_snapcraft_yaml(
    app_name: &str,
    version: &str,
    description: Option<&str>,
    runtime: &str,
    entrypoint: &[String],
    is_service: bool,
    is_desktop: bool,
) -> io::Result<SnapConfig> {
    let command = entrypoint
        .first()
        .cloned()
        .unwrap_or_else(|| runtime.to_string());

    let (plugs, command_chain, env) = desktop_config(is_desktop);

    let mut apps = BTreeMap::new();
    apps.insert(
        app_name.to_string(),
        SnapApp {
            command: Some(command.clone()),
            plugs: Some(plugs),
            command_chain,
            environment: env,
            common_id: is_desktop.then(|| format!("{app_name}.desktop")),
            desktop: is_desktop.then(|| format!("usr/share/applications/{app_name}.desktop")),
        },
    );

    let mut parts = BTreeMap::new();
    parts.insert(
        "erebus-app".to_string(),
        SnapPart {
            plugin: Some("dump".to_string()),
            source: Some(".".to_string()),
            stage: vec![],
            organize: BTreeMap::new(),
        },
    );

    Ok(SnapConfig {
        name: app_name.to_string(),
        version: version.to_string(),
        summary: Some(format!("{app_name} application")),
        description: Some(
            description
                .map(str::to_string)
                .unwrap_or_else(|| format!("{app_name} application")),
        ),
        apps: Some(apps),
        services: build_services(&command, is_service),
        parts,
        base: Some("core22".to_string()),
        confinement: Some("strict".to_string()),
        grade: Some("stable".to_string()),
        environment: if is_desktop {
            desktop_environment()
        } else {
            BTreeMap::new()
        },
        layout: desktop_layout(is_desktop),
    })
}

fn build_services(command: &str, is_service: bool) -> Option<BTreeMap<String, SnapService>> {
    if !is_service {
        return None;
    }
    let mut services = BTreeMap::new();
    services.insert(
        command.to_string(),
        SnapService {
            command: Some(command.to_string()),
            plugs: Some(cli_plugs().into_iter().map(String::from).collect()),
        },
    );
    Some(services)
}

/// Return (`plugs`, `command_chain`, `environment`) based on desktop mode.
fn desktop_config(is_desktop: bool) -> DesktopConfig {
    if is_desktop {
        (
            desktop_plugs().into_iter().map(String::from).collect(),
            Some(desktop_command_chain()),
            Some(desktop_environment()),
        )
    } else {
        (
            cli_plugs().into_iter().map(String::from).collect(),
            None,
            None,
        )
    }
}

/// Type alias for desktop configuration tuple to avoid clippy complexity.
type DesktopConfig = (
    Vec<String>,
    Option<Vec<String>>,
    Option<BTreeMap<String, String>>,
);

/// GPU layout directives for desktop apps.
fn desktop_layout(is_desktop: bool) -> Option<SnapLayout> {
    if !is_desktop {
        return None;
    }
    let mut l = BTreeMap::new();
    l.insert(
        "/usr/share/libdrm".to_string(),
        SnapLayoutEntry {
            bind: Some("$SNAP/gpu-2404/libdrm".to_string()),
            symlink: None,
            bind_file: None,
        },
    );
    l.insert(
        "/usr/share/drirc.d".to_string(),
        SnapLayoutEntry {
            bind: None,
            symlink: Some("$SNAP/gpu-2404/drirc.d".to_string()),
            bind_file: None,
        },
    );
    Some(l)
}

/// Generate the snapcraft.yaml as a string.
pub fn generate_yaml_string(config: &SnapConfig) -> io::Result<String> {
    let mut out = String::new();
    write_header(&mut out, config);
    write_environment(&mut out, &config.environment);
    write_apps(&mut out, config.apps.as_ref());
    write_services(&mut out, config.services.as_ref());
    write_parts(&mut out, &config.parts);
    write_layout(&mut out, config.layout.as_ref());
    Ok(out)
}

fn write_header(out: &mut String, config: &SnapConfig) {
    out.push_str(&format!("name: {}\n", config.name));
    out.push_str(&format!("version: {}\n", config.version));
    if let Some(summary) = &config.summary {
        out.push_str(&format!("summary: {}\n", summary));
    }
    if let Some(desc) = &config.description {
        out.push_str(&format!("description:\n  {}\n", desc));
    }
    if let Some(base) = &config.base {
        out.push_str(&format!("base: {}\n", base));
    }
    if let Some(conf) = &config.confinement {
        out.push_str(&format!("confinement: {}\n", conf));
    }
    if let Some(grade) = &config.grade {
        out.push_str(&format!("grade: {}\n", grade));
    }
}

fn write_environment(out: &mut String, env: &BTreeMap<String, String>) {
    if env.is_empty() {
        return;
    }
    out.push_str("environment:\n");
    for (k, v) in env {
        out.push_str(&format!("  {}: {}\n", k, v));
    }
}

fn write_apps(out: &mut String, apps: Option<&BTreeMap<String, SnapApp>>) {
    let Some(apps) = apps else { return };
    out.push_str("apps:\n");
    for (name, app) in apps {
        out.push_str(&format!("  {}:\n", name));
        if let Some(cmd) = &app.command {
            out.push_str(&format!("    command: {}\n", cmd));
        }
        if let Some(cid) = &app.common_id {
            out.push_str(&format!("    common-id: {}\n", cid));
        }
        if let Some(desktop) = &app.desktop {
            out.push_str(&format!("    desktop: {}\n", desktop));
        }
        if let Some(plugs) = &app.plugs {
            let plugs_str: Vec<&str> = plugs.iter().map(|s| s.as_str()).collect();
            out.push_str(&format!(
                "    plugs:\n      - {}\n",
                plugs_str.join("\n      - ")
            ));
        }
        if let Some(chain) = &app.command_chain {
            out.push_str("    command-chain:\n");
            for c in chain {
                out.push_str(&format!("      - {}\n", c));
            }
        }
        if let Some(env) = &app.environment {
            out.push_str("    environment:\n");
            for (k, v) in env {
                out.push_str(&format!("      {}: {}\n", k, v));
            }
        }
    }
}

fn write_services(out: &mut String, services: Option<&BTreeMap<String, SnapService>>) {
    let Some(services) = services else { return };
    out.push_str("services:\n");
    for (name, svc) in services {
        out.push_str(&format!("  {}:\n", name));
        if let Some(cmd) = &svc.command {
            out.push_str(&format!("    command: {}\n", cmd));
        }
        if let Some(plugs) = &svc.plugs {
            let plugs_str: Vec<&str> = plugs.iter().map(|s| s.as_str()).collect();
            out.push_str(&format!(
                "    plugs:\n      - {}\n",
                plugs_str.join("\n      - ")
            ));
        }
    }
}

fn write_parts(out: &mut String, parts: &BTreeMap<String, SnapPart>) {
    if parts.is_empty() {
        return;
    }
    out.push_str("parts:\n");
    for (name, part) in parts {
        out.push_str(&format!("  {}:\n", name));
        if let Some(plugin) = &part.plugin {
            out.push_str(&format!("    plugin: {}\n", plugin));
        }
        if let Some(source) = &part.source {
            out.push_str(&format!("    source: {}\n", source));
        }
        if !part.stage.is_empty() {
            let stages: Vec<&str> = part.stage.iter().map(|s| s.as_str()).collect();
            out.push_str(&format!(
                "    stage:\n      - {}\n",
                stages.join("\n      - ")
            ));
        }
        if !part.organize.is_empty() {
            out.push_str("    organize:\n");
            for (k, v) in &part.organize {
                out.push_str(&format!("      {}: {}\n", k, v));
            }
        }
    }
}

fn write_layout(out: &mut String, layout: Option<&SnapLayout>) {
    let Some(layout) = layout else { return };
    out.push_str("layout:\n");
    for (path, entry) in layout {
        out.push_str(&format!("  {}:\n", path));
        if let Some(bind) = &entry.bind {
            out.push_str(&format!("    bind: {}\n", bind));
        }
        if let Some(symlink) = &entry.symlink {
            out.push_str(&format!("    symlink: {}\n", symlink));
        }
        if let Some(bind_file) = &entry.bind_file {
            out.push_str(&format!("    bind-file: {}\n", bind_file));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_config_generation() {
        let config = generate_snapcraft_yaml(
            "myapp",
            "1.0.0",
            Some("A test app"),
            "python3",
            &["python3".to_string(), "/app/main.py".to_string()],
            false,
            false,
        )
        .unwrap();

        assert_eq!(config.name, "myapp");
        assert_eq!(config.version, "1.0.0");
        assert!(config.apps.is_some());
        assert!(config.apps.as_ref().unwrap().contains_key("myapp"));
    }

    #[test]
    fn snap_config_generation_desktop() {
        let config = generate_snapcraft_yaml(
            "myguiapp",
            "1.0.0",
            None,
            "python3",
            &["python3".to_string(), "/app/main.py".to_string()],
            false,
            true,
        )
        .unwrap();

        let app = config.apps.as_ref().unwrap().get("myguiapp").unwrap();
        assert!(app.command_chain.is_some());
        assert!(app.desktop.is_some());
        assert!(app.plugs.as_ref().unwrap().contains(&"x11".to_string()));
        assert!(app.plugs.as_ref().unwrap().contains(&"wayland".to_string()));
    }

    #[test]
    fn snap_service_generation() {
        let config = generate_snapcraft_yaml(
            "myapi",
            "2.0.0",
            None,
            "python3",
            &["python3".to_string(), "/app/api.py".to_string()],
            true,
            false,
        )
        .unwrap();

        assert!(config.services.is_some());
        assert!(config.apps.is_some());
    }

    #[test]
    fn yaml_string_generation() {
        let config = generate_snapcraft_yaml(
            "test",
            "1.0.0",
            Some("test"),
            "python3",
            &["python3".to_string(), "/app/main.py".to_string()],
            false,
            false,
        )
        .unwrap();

        let yaml = generate_yaml_string(&config).unwrap();
        assert!(yaml.contains("name: test"));
        assert!(yaml.contains("version: 1.0.0"));
    }

    #[test]
    fn yaml_string_generation_desktop() {
        let config = generate_snapcraft_yaml(
            "guiapp",
            "1.0.0",
            Some("GUI app"),
            "python3",
            &["python3".to_string(), "/app/main.py".to_string()],
            false,
            true,
        )
        .unwrap();

        let yaml = generate_yaml_string(&config).unwrap();
        assert!(yaml.contains("command-chain:"));
        assert!(yaml.contains("base: core22"));
        assert!(yaml.contains("GTK_USE_PORTAL"));
    }
}
