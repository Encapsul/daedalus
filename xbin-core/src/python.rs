//! PyO3 bindings — exposes format, compress, detect, pkgmgr to Python.

use pyo3::prelude::*;
use pyo3::types::PyBytes;
use std::path::Path;

use crate::assembly;
use crate::compress;
use crate::detect::{self, Runtime};
use crate::format::{self, Footer};
use crate::pkgmgr::{self, PkgMgr};

// ─── format ──────────────────────────────────────────────────────────────

#[pyclass]
#[derive(Debug, Clone)]
struct PyFooter {
    #[pyo3(get)]
    format_version: u8,
    #[pyo3(get)]
    arch: u8,
    #[pyo3(get)]
    flags: u8,
    #[pyo3(get)]
    payload_offset: u64,
    #[pyo3(get)]
    payload_csize: u64,
    #[pyo3(get)]
    payload_usize: u64,
    #[pyo3(get)]
    payload_sha256: Vec<u8>,
    #[pyo3(get)]
    meta_offset: u64,
    #[pyo3(get)]
    meta_size: u64,
    #[pyo3(get)]
    sig_offset: u64,
}

#[pymethods]
impl PyFooter {
    #[new]
    fn py_new(_py: Python<'_>) -> PyResult<Self> {
        Err(pyo3::exceptions::PyNotImplementedError::new_err(
            "Use PyFooter.unpack() to create a PyFooter from packed data",
        ))
    }

    #[getter]
    fn footer_size(&self) -> u64 {
        if self.format_version >= 3 {
            format::V3_FOOTER_SIZE
        } else {
            format::V2_FOOTER_SIZE
        }
    }

    #[getter]
    fn crypto_suite(&self) -> u64 {
        if self.format_version >= 4 {
            self.payload_usize
        } else {
            format::CRYPTO_NONE
        }
    }

    #[setter]
    fn set_crypto_suite(&mut self, value: u64) {
        if self.format_version >= 4 {
            self.payload_usize = value;
        }
    }

    fn is_signed(&self) -> bool {
        self.flags & format::FLAG_SIGNED != 0
    }

    fn sha256_hex(&self) -> String {
        self.payload_sha256
            .iter()
            .fold(String::with_capacity(64), |mut s, b| {
                use std::fmt::Write;
                let _ = write!(s, "{b:02x}");
                s
            })
    }

    fn pack<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let total = if self.format_version >= 3 {
            format::V3_FOOTER_SIZE as usize
        } else {
            format::V2_FOOTER_SIZE as usize
        };
        let mut buf = vec![0u8; total];

        if self.format_version >= 3 {
            buf[0..8].copy_from_slice(&self.sig_offset.to_le_bytes());
            let core = &mut buf[8..];
            core[0..5].copy_from_slice(format::MAGIC);
            core[5] = self.format_version;
            core[6] = self.arch;
            core[7] = self.flags;
            core[8..16].copy_from_slice(&self.payload_offset.to_le_bytes());
            core[16..24].copy_from_slice(&self.payload_csize.to_le_bytes());
            core[24..32].copy_from_slice(&self.payload_usize.to_le_bytes());
            core[32..64].copy_from_slice(&self.payload_sha256);
            core[64..72].copy_from_slice(&self.meta_offset.to_le_bytes());
            core[72..80].copy_from_slice(&self.meta_size.to_le_bytes());
            core[80..84].copy_from_slice(&format::FOOTER_MAGIC.to_le_bytes());
        } else {
            buf[0..5].copy_from_slice(format::MAGIC);
            buf[5] = self.format_version;
            buf[6] = self.arch;
            buf[7] = self.flags;
            buf[8..16].copy_from_slice(&self.payload_offset.to_le_bytes());
            buf[16..24].copy_from_slice(&self.payload_csize.to_le_bytes());
            buf[24..32].copy_from_slice(&self.payload_usize.to_le_bytes());
            buf[32..64].copy_from_slice(&self.payload_sha256);
            buf[64..72].copy_from_slice(&self.meta_offset.to_le_bytes());
            buf[72..80].copy_from_slice(&self.meta_size.to_le_bytes());
            buf[80..84].copy_from_slice(&format::FOOTER_MAGIC.to_le_bytes());
        }

        Ok(PyBytes::new(py, &buf))
    }

    #[staticmethod]
    fn unpack(data: &[u8]) -> PyResult<Self> {
        let mut sig_offset = 0u64;
        let core = if data.len() == format::V3_FOOTER_SIZE as usize {
            sig_offset = u64::from_le_bytes(
                data[0..8]
                    .try_into()
                    .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?,
            );
            &data[8..]
        } else if data.len() == format::V2_FOOTER_SIZE as usize {
            data
        } else {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "footer must be {} or {} bytes, got {}",
                format::V2_FOOTER_SIZE,
                format::V3_FOOTER_SIZE,
                data.len()
            )));
        };

        if &core[0..5] != format::MAGIC {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "bad magic: not a .xbin file",
            ));
        }
        let footer_magic = u32::from_le_bytes(
            core[80..84]
                .try_into()
                .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?,
        );
        if footer_magic != format::FOOTER_MAGIC {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "bad footer sentinel",
            ));
        }

        let mut sha = [0u8; 32];
        sha.copy_from_slice(&core[32..64]);

        Ok(Self {
            format_version: core[5],
            arch: core[6],
            flags: core[7],
            payload_offset: u64::from_le_bytes(
                core[8..16]
                    .try_into()
                    .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?,
            ),
            payload_csize: u64::from_le_bytes(
                core[16..24]
                    .try_into()
                    .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?,
            ),
            payload_usize: u64::from_le_bytes(
                core[24..32]
                    .try_into()
                    .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?,
            ),
            payload_sha256: sha.to_vec(),
            meta_offset: u64::from_le_bytes(
                core[64..72]
                    .try_into()
                    .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?,
            ),
            meta_size: u64::from_le_bytes(
                core[72..80]
                    .try_into()
                    .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?,
            ),
            sig_offset,
        })
    }

    fn __repr__(&self) -> String {
        format!("Footer(version={}, arch={:#04x}, flags={:#04x}, payload_offset={}, payload_csize={}, payload_usize={}, meta_offset={}, meta_size={}, sig_offset={})", self.format_version, self.arch, self.flags, self.payload_offset, self.payload_csize, self.payload_usize, self.meta_offset, self.meta_size, self.sig_offset,)
    }
}

#[pyfunction]
fn py_read_footer(path: &str) -> PyResult<PyFooter> {
    let mut f = std::fs::File::open(path)
        .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?;
    let footer = Footer::read_from(&mut f)
        .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?;
    Ok(PyFooter {
        format_version: footer.format_version,
        arch: footer.arch,
        flags: footer.flags,
        payload_offset: footer.payload_offset,
        payload_csize: footer.payload_csize,
        payload_usize: footer.payload_usize,
        payload_sha256: footer.payload_sha256.to_vec(),
        meta_offset: footer.meta_offset,
        meta_size: footer.meta_size,
        sig_offset: footer.sig_offset,
    })
}

#[pyfunction]
fn py_read_at<'py>(
    py: Python<'py>,
    path: &str,
    offset: u64,
    length: usize,
) -> PyResult<Bound<'py, PyBytes>> {
    let mut f = std::fs::File::open(path)
        .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?;
    let data = format::read_at(&mut f, offset, length)
        .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?;
    Ok(PyBytes::new(py, &data))
}

// ─── compress ────────────────────────────────────────────────────────────

#[pyfunction]
#[pyo3(signature = (data, level=None))]
fn py_compress<'py>(
    py: Python<'py>,
    data: &[u8],
    level: Option<i32>,
) -> PyResult<Bound<'py, PyBytes>> {
    let lv = level.unwrap_or(3);
    let compressed = compress::compress_with_level(data, lv)
        .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?;
    Ok(PyBytes::new(py, &compressed))
}

#[pyfunction]
fn py_decompress<'py>(py: Python<'py>, data: &[u8]) -> PyResult<Bound<'py, PyBytes>> {
    let decompressed = compress::decompress(data)
        .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?;
    Ok(PyBytes::new(py, &decompressed))
}

// ─── detect ──────────────────────────────────────────────────────────────

#[pyfunction]
fn py_detect_runtime(app_dir: &str) -> Option<String> {
    detect::detect_runtime(Path::new(app_dir)).map(|r| r.name().to_string())
}

#[pyfunction]
fn py_detect_python(app_dir: &str) -> bool {
    detect::detect_runtime(Path::new(app_dir)) == Some(Runtime::Python)
}

#[pyfunction]
fn py_detect_deno(app_dir: &str) -> bool {
    detect::detect_runtime(Path::new(app_dir)) == Some(Runtime::Deno)
}

#[pyfunction]
fn py_detect_node(app_dir: &str) -> bool {
    detect::detect_runtime(Path::new(app_dir)) == Some(Runtime::Node)
}

#[pyfunction]
fn py_detect_java(app_dir: &str) -> bool {
    detect::detect_runtime(Path::new(app_dir)) == Some(Runtime::Java)
}

#[pyfunction]
fn py_detect_ruby(app_dir: &str) -> bool {
    detect::detect_runtime(Path::new(app_dir)) == Some(Runtime::Ruby)
}

#[pyfunction]
fn py_detect_dotnet(app_dir: &str) -> bool {
    detect::detect_runtime(Path::new(app_dir)) == Some(Runtime::Dotnet)
}

#[pyfunction]
fn py_detect_go(app_dir: &str) -> bool {
    detect::detect_runtime(Path::new(app_dir)) == Some(Runtime::Go)
}

#[pyfunction]
fn py_detect_php(app_dir: &str) -> bool {
    detect::detect_runtime(Path::new(app_dir)) == Some(Runtime::Php)
}

#[pyfunction]
fn py_detect_perl(app_dir: &str) -> bool {
    detect::detect_runtime(Path::new(app_dir)) == Some(Runtime::Perl)
}

#[pyfunction]
fn py_detect_electron(app_dir: &str) -> bool {
    detect::detect_runtime(Path::new(app_dir)) == Some(Runtime::Electron)
}

#[pyfunction]
fn py_detect_binary(app_dir: &str) -> bool {
    detect::detect_runtime(Path::new(app_dir)) == Some(Runtime::Binary)
}

// ─── pkgmgr ──────────────────────────────────────────────────────────────

#[pyfunction]
fn py_detect_python_pkgmgr(dir: &str) -> Option<String> {
    pkgmgr::detect_python_pkgmgr(Path::new(dir)).map(|p| p.name().to_string())
}

#[pyfunction]
fn py_detect_node_pkgmgr(dir: &str) -> Option<String> {
    pkgmgr::detect_node_pkgmgr(Path::new(dir)).map(|p| p.name().to_string())
}

#[pyfunction]
fn py_detect_pkgmgr(dir: &str, runtime: &str) -> Option<String> {
    pkgmgr::detect_pkgmgr(Path::new(dir), runtime).map(|p| p.name().to_string())
}

#[pyfunction]
fn py_pkgmgr_install_cmd(mgr: &str) -> Vec<String> {
    let name = match mgr {
        "uv" => Some(PkgMgr::Uv),
        "poetry" => Some(PkgMgr::Poetry),
        "pipenv" => Some(PkgMgr::Pipenv),
        "pip" => Some(PkgMgr::Pip),
        "pnpm" => Some(PkgMgr::Pnpm),
        "yarn" => Some(PkgMgr::Yarn),
        "bun" => Some(PkgMgr::Bun),
        "npm" => Some(PkgMgr::Npm),
        _ => None,
    };
    name.map(|p| p.install_cmd().into_iter().map(String::from).collect())
        .unwrap_or_default()
}

// ─── tar ─────────────────────────────────────────────────────────────────

#[pyfunction]
fn py_create_tar<'py>(py: Python<'py>, root: &str) -> PyResult<Bound<'py, PyBytes>> {
    let tar_bytes = crate::tar::create_deterministic_tar(Path::new(root))
        .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?;
    Ok(PyBytes::new(py, &tar_bytes))
}

#[pyfunction]
fn py_create_tar_zstd<'py>(py: Python<'py>, root: &str) -> PyResult<Bound<'py, PyBytes>> {
    let compressed = crate::tar::create_tar_zstd(Path::new(root))
        .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?;
    Ok(PyBytes::new(py, &compressed))
}

// ─── assembly ────────────────────────────────────────────────────────────

#[pyfunction]
#[pyo3(signature = (out_path, stub_bytes, payload, meta_bytes, encrypt=false, squashfs=false, target_arch=None))]
fn py_assemble_xbin(
    out_path: &str,
    stub_bytes: &[u8],
    payload: &[u8],
    meta_bytes: &[u8],
    encrypt: bool,
    squashfs: bool,
    target_arch: Option<&str>,
) -> PyResult<u64> {
    let size = assembly::assemble_xbin(
        Path::new(out_path),
        stub_bytes,
        payload,
        meta_bytes,
        encrypt,
        squashfs,
        target_arch,
    )
    .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?;
    Ok(size)
}

// ─── module ──────────────────────────────────────────────────────────────

#[pymodule]
fn xbin_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // format
    m.add("MAGIC", format::MAGIC)?;
    m.add("FOOTER_MAGIC", format::FOOTER_MAGIC)?;
    m.add("FORMAT_VERSION", format::FORMAT_VERSION)?;
    m.add("V2_FOOTER_SIZE", format::V2_FOOTER_SIZE)?;
    m.add("V3_FOOTER_SIZE", format::V3_FOOTER_SIZE)?;
    m.add("CRYPTO_NONE", format::CRYPTO_NONE)?;
    m.add("CRYPTO_AES_256_GCM", format::CRYPTO_AES_256_GCM)?;
    m.add("FLAG_SIGNED", format::FLAG_SIGNED)?;
    m.add("FLAG_ENCRYPTED", format::FLAG_ENCRYPTED)?;
    m.add("ARCH_X86_64", format::ARCH_X86_64)?;
    m.add("ARCH_AARCH64", format::ARCH_AARCH64)?;
    m.add_class::<PyFooter>()?;
    m.add_function(wrap_pyfunction!(py_read_footer, m)?)?;
    m.add_function(wrap_pyfunction!(py_read_at, m)?)?;

    // compress
    m.add_function(wrap_pyfunction!(py_compress, m)?)?;
    m.add_function(wrap_pyfunction!(py_decompress, m)?)?;

    // detect
    m.add_function(wrap_pyfunction!(py_detect_runtime, m)?)?;
    m.add_function(wrap_pyfunction!(py_detect_python, m)?)?;
    m.add_function(wrap_pyfunction!(py_detect_deno, m)?)?;
    m.add_function(wrap_pyfunction!(py_detect_node, m)?)?;
    m.add_function(wrap_pyfunction!(py_detect_java, m)?)?;
    m.add_function(wrap_pyfunction!(py_detect_ruby, m)?)?;
    m.add_function(wrap_pyfunction!(py_detect_dotnet, m)?)?;
    m.add_function(wrap_pyfunction!(py_detect_go, m)?)?;
    m.add_function(wrap_pyfunction!(py_detect_php, m)?)?;
    m.add_function(wrap_pyfunction!(py_detect_perl, m)?)?;
    m.add_function(wrap_pyfunction!(py_detect_binary, m)?)?;

    // pkgmgr
    m.add_function(wrap_pyfunction!(py_detect_python_pkgmgr, m)?)?;
    m.add_function(wrap_pyfunction!(py_detect_node_pkgmgr, m)?)?;
    m.add_function(wrap_pyfunction!(py_detect_pkgmgr, m)?)?;
    m.add_function(wrap_pyfunction!(py_pkgmgr_install_cmd, m)?)?;

    // tar
    m.add_function(wrap_pyfunction!(py_create_tar, m)?)?;
    m.add_function(wrap_pyfunction!(py_create_tar_zstd, m)?)?;

    // assembly
    m.add_function(wrap_pyfunction!(py_assemble_xbin, m)?)?;

    Ok(())
}
