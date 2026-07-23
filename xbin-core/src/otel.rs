use std::collections::HashMap;
use std::env;

pub fn build_otel_env(
    service_name: &str,
    version: &str,
    endpoint: &str,
    protocol: &str,
    traces_exporter: &str,
    metrics_exporter: &str,
    logs_exporter: &str,
) -> HashMap<String, String> {
    let mut env = HashMap::new();

    env.insert("OTEL_SERVICE_NAME".to_string(), service_name.to_string());
    env.insert(
        "OTEL_TRACES_EXPORTER".to_string(),
        traces_exporter.to_string(),
    );
    env.insert(
        "OTEL_METRICS_EXPORTER".to_string(),
        metrics_exporter.to_string(),
    );
    env.insert("OTEL_LOGS_EXPORTER".to_string(), logs_exporter.to_string());

    let mut resource_attrs = format!("service.name={service_name}");
    if !version.is_empty() {
        resource_attrs.push_str(&format!(",service.version={version}"));
    }
    resource_attrs.push_str(",deployment.mode=server");
    env.insert("OTEL_RESOURCE_ATTRIBUTES".to_string(), resource_attrs);

    if !endpoint.is_empty() {
        env.insert(
            "OTEL_EXPORTER_OTLP_ENDPOINT".to_string(),
            endpoint.to_string(),
        );
        env.insert(
            "OTEL_EXPORTER_OTLP_PROTOCOL".to_string(),
            protocol.to_string(),
        );
    }

    if traces_exporter != "none" || metrics_exporter != "none" {
        env.insert(
            "OTEL_PYTHON_AUTO_INSTRUMENTATION_ENABLED".to_string(),
            "true".to_string(),
        );
    }

    env
}

pub fn get_otel_config() -> HashMap<String, String> {
    let otel_vars = [
        "OTEL_SERVICE_NAME",
        "OTEL_RESOURCE_ATTRIBUTES",
        "OTEL_EXPORTER_OTLP_ENDPOINT",
        "OTEL_EXPORTER_OTLP_PROTOCOL",
        "OTEL_TRACES_EXPORTER",
        "OTEL_METRICS_EXPORTER",
        "OTEL_LOGS_EXPORTER",
        "OTEL_PYTHON_AUTO_INSTRUMENTATION_ENABLED",
    ];
    let mut config = HashMap::new();
    for var in otel_vars {
        if let Ok(val) = env::var(var) {
            config.insert(var.to_string(), val);
        }
    }
    config
}

pub fn format_resource_attributes(attrs_str: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    if attrs_str.is_empty() {
        return result;
    }
    for pair in attrs_str.split(',') {
        if let Some((key, value)) = pair.split_once('=') {
            result.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_otel_env_minimal() {
        let env = build_otel_env("myapp", "", "", "grpc", "otlp", "otlp", "none");
        assert_eq!(env.get("OTEL_SERVICE_NAME").unwrap(), "myapp");
        assert_eq!(env.get("OTEL_TRACES_EXPORTER").unwrap(), "otlp");
        assert_eq!(env.get("OTEL_METRICS_EXPORTER").unwrap(), "otlp");
        assert_eq!(env.get("OTEL_LOGS_EXPORTER").unwrap(), "none");
        let attrs = env.get("OTEL_RESOURCE_ATTRIBUTES").unwrap();
        assert!(attrs.contains("service.name=myapp"));
        assert!(attrs.contains("deployment.mode=server"));
        assert!(!attrs.contains("service.version"));
        assert!(!env.contains_key("OTEL_EXPORTER_OTLP_ENDPOINT"));
        assert_eq!(
            env.get("OTEL_PYTHON_AUTO_INSTRUMENTATION_ENABLED").unwrap(),
            "true"
        );
    }

    #[test]
    fn test_build_otel_env_with_endpoint() {
        let env = build_otel_env(
            "svc",
            "1.0",
            "http://localhost:4317",
            "grpc",
            "otlp",
            "otlp",
            "none",
        );
        assert_eq!(
            env.get("OTEL_EXPORTER_OTLP_ENDPOINT").unwrap(),
            "http://localhost:4317"
        );
        assert_eq!(env.get("OTEL_EXPORTER_OTLP_PROTOCOL").unwrap(), "grpc");
        let attrs = env.get("OTEL_RESOURCE_ATTRIBUTES").unwrap();
        assert!(attrs.contains("service.version=1.0"));
    }

    #[test]
    fn test_format_resource_attributes() {
        let result = format_resource_attributes("a=1,b=2,c=3");
        assert_eq!(result.get("a").unwrap(), "1");
        assert_eq!(result.get("b").unwrap(), "2");
        assert_eq!(result.get("c").unwrap(), "3");
    }

    #[test]
    fn test_format_resource_attributes_empty() {
        assert!(format_resource_attributes("").is_empty());
    }
}
