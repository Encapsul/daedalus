//! GPU / accelerated-compute detection for AI workloads.
//!
//! Used by `daedalus build --gpu auto` to pick a compute backend at build
//! time (host build → target host). Detection is cheap: existence checks on
//! well-known kernel/device interfaces, never spawning driver tooling.

use std::path::{Path, PathBuf};

/// Compute backends daedalus knows how to pass through to packaged apps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuBackend {
    /// NVIDIA CUDA / OptiX (the launcher bind-mounts `/dev/nvidia*`).
    Nvidia,
    /// AMD ROCm (the launcher bind-mounts `/dev/kfd`, `/dev/dri/renderD*`).
    Rocm,
}

impl GpuBackend {
    /// Machine-readable backend name written to the binary metadata.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Nvidia => "nvidia",
            Self::Rocm => "rocm",
        }
    }
}

impl std::fmt::Display for GpuBackend {
    /// `fmt` - display.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Result of a GPU probe on the build host.
#[derive(Debug)]
pub struct GpuInfo {
    /// Detected backend, or `None` when no accelerator is present.
    pub backend: Option<GpuBackend>,
    /// Number of visible devices of the detected backend.
    pub device_count: u32,
    /// Host paths that will carry the compute at runtime.
    pub devices: Vec<PathBuf>,
}

/// Probe the host for an NVIDIA or AMD ROCm accelerator.
///
/// NVIDIA is reported when the kernel driver directory
/// `/proc/driver/nvidia/gpus` is populated (avoids false positives on
/// straggler device nodes). ROCm wins when both are present: `/dev/kfd`
/// gates the whole driver, so its existence is the primary signal.
pub fn detect_gpu() -> GpuInfo {
    if let Some(info) = probe_rocm() {
        return info;
    }
    if let Some((count, devices)) = probe_nvidia() {
        return GpuInfo {
            backend: Some(GpuBackend::Nvidia),
            device_count: count,
            devices,
        };
    }
    GpuInfo {
        backend: None,
        device_count: 0,
        devices: Vec::new(),
    }
}

/// Returns `(device_count, device node paths)` or `None` when the NVIDIA
/// driver is not active on this host.
fn probe_nvidia() -> Option<(u32, Vec<PathBuf>)> {
    let proc_dir = Path::new("/proc/driver/nvidia/gpus");
    let dev_dir = Path::new("/dev");
    if !proc_dir.is_dir() {
        return None;
    }
    let gpus = read_dir_first(proc_dir)?;
    if gpus.is_empty() {
        return None;
    }
    let dev_nodes = read_dir_first(dev_dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|p| nvidia_device_node(p))
        .collect();
    Some((u32::try_from(gpus.len()).unwrap_or(u32::MAX), dev_nodes))
}

/// Is this `/dev` entry an NVIDIA device node (`nvidia*`, minus the caps dir)?
fn nvidia_device_node(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.starts_with("nvidia") && name != "nvidia-caps"
}

/// Returns a ROCm probe result or `None` when the driver is absent.
fn probe_rocm() -> Option<GpuInfo> {
    if !Path::new("/dev/kfd").exists() {
        return None;
    }
    let mut devices = vec![PathBuf::from("/dev/kfd")];
    if let Some(dri) = read_dir_first(Path::new("/dev/dri")) {
        let nodes = dri
            .into_iter()
            .filter(|p| {
                let name = p
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default();
                name.starts_with("renderD") || name.starts_with("card")
            })
            .collect::<Vec<_>>();
        let count = u32::try_from(nodes.len()).unwrap_or(1);
        devices.extend(nodes);
        Some(GpuInfo {
            backend: Some(GpuBackend::Rocm),
            device_count: count,
            devices,
        })
    } else {
        Some(GpuInfo {
            backend: Some(GpuBackend::Rocm),
            device_count: 1,
            devices,
        })
    }
}

/// Best-effort flattened directory listing; `None` when unreadable.
fn read_dir_first(dir: &Path) -> Option<Vec<PathBuf>> {
    std::fs::read_dir(dir)
        .ok()
        .map(|it| it.flatten().map(|e| e.path()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// `backend_as_str_is_machine_readable` - backend names.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn backend_as_str_is_machine_readable() {
        assert_eq!(GpuBackend::Nvidia.as_str(), "nvidia");
        assert_eq!(GpuBackend::Rocm.as_str(), "rocm");
        assert_eq!(GpuBackend::Nvidia.to_string(), "nvidia");
    }

    #[test]
    /// `probe_rocm_needs_kfd` - kfd presence gates ROCm.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn probe_rocm_needs_kfd() {
        assert!(probe_rocm().is_none());
    }

    #[test]
    /// `detect_gpu_defaults_to_none` - no-accelerator host fallback.
    ///
    /// Description:
    ///
    /// Return: nothing
    fn detect_gpu_defaults_to_none() {
        assert!(detect_gpu().backend.is_none());
    }
}
