//! Snap packaging support.
//!
//! Generates `snapcraft.yaml` from erebus artifact metadata, enabling
//! `snapcraft` to produce distributable Snap packages.

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapApp {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugs: Option<Vec<String>>,
    #[serde(rename = "command-chain", skip_serializing_if = "Option::is_none")]
    pub command_chain: Option<Vec<String>>,
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

/// Generate a snapcraft.yaml from artifact metadata.
pub fn generate_snapcraft_yaml(
    app_name: &str,
    version: &str,
    description: Option<&str>,
    runtime: &str,
    entrypoint: &[String],
    is_service: bool,
) -> io::Result<SnapConfig> {
    let command = entrypoint
        .first()
        .cloned()
        .unwrap_or_else(|| runtime.to_string());

    let mut apps = BTreeMap::new();
    apps.insert(
        app_name.to_string(),
        SnapApp {
            command: Some(command.clone()),
            plugs: Some(vec!["network".to_string(), "home".to_string()]),
            command_chain: None,
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
        description: description.map(str::to_string),
        apps: Some(apps),
        services: if is_service {
            let mut services = BTreeMap::new();
            services.insert(
                app_name.to_string(),
                SnapService {
                    command: Some(command),
                    plugs: Some(vec!["network".to_string()]),
                },
            );
            Some(services)
        } else {
            None
        },
        parts,
        base: Some("core22".to_string()),
        confinement: Some("strict".to_string()),
        grade: Some("stable".to_string()),
    })
}

/// Generate the snapcraft.yaml as a string.
pub fn generate_yaml_string(config: &SnapConfig) -> io::Result<String> {
    let mut out = String::new();
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
    out.push_str("apps:\n");
    if let Some(apps) = &config.apps {
        for (name, app) in apps {
            out.push_str(&format!("  {}:\n", name));
            if let Some(cmd) = &app.command {
                out.push_str(&format!("    command: {}\n", cmd));
            }
            if let Some(plugs) = &app.plugs {
                let plugs_str: Vec<&str> = plugs.iter().map(|s| s.as_str()).collect();
                out.push_str(&format!(
                    "    plugs:\n      - {}\n",
                    plugs_str.join("\n      - ")
                ));
            }
        }
    }
    if let Some(services) = &config.services {
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
    out.push_str("parts:\n");
    for (name, part) in &config.parts {
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
    Ok(out)
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
        )
        .unwrap();

        assert_eq!(config.name, "myapp");
        assert_eq!(config.version, "1.0.0");
        assert!(config.apps.is_some());
        assert!(config.apps.as_ref().unwrap().contains_key("myapp"));
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
        )
        .unwrap();

        let yaml = generate_yaml_string(&config).unwrap();
        assert!(yaml.contains("name: test"));
        assert!(yaml.contains("version: 1.0.0"));
    }
}
