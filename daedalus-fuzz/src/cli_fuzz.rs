//! CLI fuzzing target - runs CLI as subprocess.

use crate::FuzzTarget;
use anyhow::{Context, Result};
use arbitrary::Unstructured;
use std::process::Command;

pub struct CliFuzzTarget;

impl CliFuzzTarget {
    fn generate_random_args(&self, unstructured: &mut Unstructured) -> Result<Vec<String>> {
        let commands = [
            "build", "serve", "pull", "publish", "sign", "verify", "keygen", "inspect", "scan",
            "clean", "upgrade", "doctor", "env", "trust", "run",
        ];
        let cmd = *unstructured.choose(&commands)?;
        let mut args = vec![cmd.to_string()];
        let flag_count = unstructured.int_in_range(0..=3)?;
        let flags = [
            "--help",
            "--version",
            "--verbose",
            "--quiet",
            "--json",
            "--output",
            "/tmp/test.de",
            "--port",
            "8080",
            "--registry",
            "http://localhost:8080",
            "--token",
            "test",
        ];
        for _ in 0..flag_count {
            args.push(unstructured.choose(&flags)?.to_string());
        }
        Ok(args)
    }
}

impl FuzzTarget for CliFuzzTarget {
    fn name(&self) -> &'static str {
        "cli"
    }
    fn generate_seed(&self, unstructured: &mut Unstructured) -> Result<Vec<u8>> {
        let args = self.generate_random_args(unstructured)?;
        Ok(serde_json::to_vec(&serde_json::json!({ "argv": args }))?)
    }
    fn mutate(&self, input: &[u8], unstructured: &mut Unstructured) -> Result<Vec<u8>> {
        let mut seed: serde_json::Value = serde_json::from_slice(input)?;
        if let Some(argv) = seed.get_mut("argv").and_then(|v| v.as_array_mut()) {
            let mut args: Vec<String> = argv
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            if !args.is_empty() && unstructured.ratio(1, 2)? {
                let idx = unstructured.int_in_range(0..=args.len() - 1)?;
                args[idx] = unstructured.arbitrary::<String>()?;
            }
            argv.clear();
            argv.extend(args.into_iter().map(serde_json::Value::String));
        }
        Ok(serde_json::to_vec(&seed)?)
    }
    fn execute(&self, input: &[u8]) -> Result<()> {
        let seed: serde_json::Value = serde_json::from_slice(input)?;
        let argv: Vec<String> = seed
            .get("argv")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        if argv.is_empty() || argv[0] != "daedalus" {
            return Ok(());
        }
        let daedalus_bin = std::env::var("DAEDALUS_BIN").unwrap_or_else(|_| "daedalus".to_string());
        let _output = Command::new(&daedalus_bin)
            .args(&argv)
            .output()
            .context("failed to run daedalus")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_cli_target_creation() {
        let target = CliFuzzTarget;
        assert_eq!(target.name(), "cli");
    }
}
