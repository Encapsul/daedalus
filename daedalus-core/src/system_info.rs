//! System information detection for build-time configuration adaptation.
//!
//! Detects hardware resources (RAM, CPU cores) to adapt build parameters
//! such as universal binary slice counts, lazy-loading priorities, etc.

use std::fs;

/// Detected system hardware configuration.
#[derive(Debug, Clone, Default)]
pub struct SystemConfig {
    /// Total RAM in bytes
    pub total_memory: u64,
    /// Number of logical CPU cores
    pub cpu_cores: usize,
    /// Architecture string (e.g., "x86_64", "aarch64")
    pub architecture: String,
}

/// Detect the system's hardware configuration.
///
/// Uses OS-specific methods to determine available resources.
/// Falls back to conservative defaults if detection fails.
pub fn detect() -> SystemConfig {
    // Try to read from /proc/meminfo (Linux) or similar paths
    let total_memory = detect_memory();
    let cpu_cores = detect_cpu_cores();
    let architecture = detect_architecture();

    SystemConfig {
        total_memory,
        cpu_cores,
        architecture,
    }
}

/// Detect total available memory in bytes.
fn detect_memory() -> u64 {
    // Try Linux /proc/meminfo first
    if let Ok(content) = fs::read_to_string("/proc/meminfo") {
        for line in content.lines() {
            if line.starts_with("MemTotal:") {
                if let Some(value) = line.split_whitespace().nth(1) {
                    if let Ok(bytes) = value.parse::<u64>() {
                        return bytes;
                    }
                }
            }
        }
    }

    // Fallback: use a conservative estimate (512MB)
    512 * 1024 * 1024
}

/// Detect the number of logical CPU cores.
fn detect_cpu_cores() -> usize {
    num_cpus::get()
}

/// Detect the system architecture string.
fn detect_architecture() -> String {
    std::env::consts::ARCH.to_string()
}

/// Return the total memory in a human-readable format.
pub fn format_memory(memory: u64) -> String {
    const GIGABYTE: u64 = 1024 * 1024 * 1024;
    const MEAGABYTE: u64 = 1024 * 1024;

    if memory >= GIGABYTE {
        format!("{} GiB", memory / GIGABYTE)
    } else if memory >= MEAGABYTE {
        format!("{} MiB", memory / MEAGABYTE)
    } else {
        format!("{} Ko", memory / 1024)
    }
}

/// Adapt universal binary slice count based on system memory and CPU cores.
/// More slices for systems with more RAM and cores, for better parallelism.
///
/// Slice count mapping:
/// - < 2GB RAM & < 2 cores: 4 slices (minimum)
/// - 2GB-3GB RAM & 2-3 cores: 4 slices
/// - 4GB-7GB RAM & 4 cores: 5 slices
/// - 8GB+ RAM & 8 cores: 8 slices
pub fn compute_universal_slices(memory_bytes: u64, cpu_cores: usize) -> usize {
    let cores = cpu_cores.max(1);

    // Determine slice count based on memory tier
    let memory_tier = if memory_bytes >= 8 * 1024 * 1024 * 1024 {
        // 8GB+: 8 slices
        8
    } else if memory_bytes >= 4 * 1024 * 1024 * 1024 {
        // 4GB-7GB: 5 slices
        5
    } else {
        // <4GB: 4 slices (minimum)
        4
    };

    // Use the greater of memory-tier slices and cores, capped at 8
    memory_tier.max(cores).min(8)
}

/// Resource thresholds below which background work is not worth the CPU it
/// steals from the app. A host with fewer cores or less RAM than these runs
/// lazy extraction up-front instead, where the whole decompression happens
/// once and the app never competes with a background thread.
pub const CONSTRAINED_CORES: usize = 4;
pub const CONSTRAINED_MEMORY_BYTES: u64 = 3 * 1024 * 1024 * 1024;

/// Whether the detected host is too small for background (lazy) extraction.
///
/// Background decompression on a 2-core box stalls the app it is meant to
/// get started faster; such hosts extract the full payload up-front.
pub fn is_constrained_host(config: &SystemConfig) -> bool {
    config.cpu_cores < CONSTRAINED_CORES
        || (config.total_memory > 0 && config.total_memory < CONSTRAINED_MEMORY_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_returns_config() {
        let config = detect();
        assert!(!config.architecture.is_empty());
        assert!(config.cpu_cores > 0);
        assert!(config.total_memory > 0);
    }

    #[test]
    fn test_format_memory() {
        // 1GB should format as "1 GiB"
        assert_eq!(format_memory(1024 * 1024 * 1024), "1 GiB");
        // 500MB should format as "512 MiB"
        assert_eq!(format_memory(512 * 1024 * 1024), "512 MiB");
        // 1KB should format as "1 Ko"
        assert_eq!(format_memory(1024), "1 Ko");
    }

    #[test]
    fn test_compute_slices() {
        // 2GB RAM & 2 cores → 4 slices (minimum)
        assert_eq!(compute_universal_slices(2 * 1024 * 1024 * 1024, 2), 4);
        // 4GB RAM & 4 cores → 5 slices
        assert_eq!(compute_universal_slices(4 * 1024 * 1024 * 1024, 4), 5);
        // 8GB+ RAM & 8 cores → 8 slices
        assert_eq!(compute_universal_slices(16 * 1024 * 1024 * 1024, 8), 8);
    }

    #[test]
    fn test_is_constrained_host() {
        let cfg = SystemConfig {
            total_memory: 3 * 1024 * 1024 * 1024,
            cpu_cores: 4,
            architecture: "x86_64".into(),
        };
        assert!(!is_constrained_host(&cfg));
        // Few cores
        assert!(is_constrained_host(&SystemConfig {
            total_memory: 16 * 1024 * 1024 * 1024,
            cpu_cores: 2,
            architecture: "x86_64".into(),
        }));
        // Low RAM equals the threshold → not constrained
        assert!(!is_constrained_host(&SystemConfig {
            total_memory: 3 * 1024 * 1024 * 1024,
            cpu_cores: 8,
            architecture: "aarch64".into(),
        }));
        // Missing memory detection (0) never counts as constrained on its own
        assert!(!is_constrained_host(&SystemConfig {
            total_memory: 0,
            cpu_cores: 8,
            architecture: "x86_64".into(),
        }));
    }
}
