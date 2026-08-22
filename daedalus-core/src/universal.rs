//! Universal binary format for daedalus.
//!
//! A universal `.daedalus` file embeds multiple architecture-specific
//! slices behind a polyglot shell-script launcher. The launcher detects
//! `uname -m`/`uname -s` at runtime (using hardcoded offsets generated
//! at build time), extracts the matching slice to a temp file, and
//! `exec`s it. Each slice is a complete `.daedalus` binary.
//!
//! Layout:
//! ```text
//! [shell-script launcher  (page-aligned to SIG_BLOCK_SIZE)]
//! [linux-x86_64 slice]
//! [linux-aarch64 slice]
//! [optional slices]
//! [universal footer    (26 B)]
//! ```

use std::io::{self, Write};

/// Magic for the universal footer.
pub const UNIV_FOOTER_MAGIC: u32 = 0xBEEF_CABE;

/// Universal footer: the last 26 bytes of a universal `.daedalus`.
/// Allows tools to inspect a universal binary without running it.
#[derive(Debug, Clone)]
pub struct UniversalFooter {
    pub magic: u32,
    pub num_slices: u32,
    pub manifest_offset: u64,
    pub manifest_size: u32,
    pub reserved: u32,
}

impl UniversalFooter {
    pub const SIZE: usize = 26;

    pub fn pack(&self) -> [u8; Self::SIZE] {
        let mut out = [0u8; Self::SIZE];
        out[0..4].copy_from_slice(&self.magic.to_le_bytes());
        out[4..8].copy_from_slice(&self.num_slices.to_le_bytes());
        out[8..16].copy_from_slice(&self.manifest_offset.to_le_bytes());
        out[16..20].copy_from_slice(&self.manifest_size.to_le_bytes());
        out[20..24].copy_from_slice(&self.reserved.to_le_bytes());
        out
    }

    pub fn parse(buf: &[u8]) -> io::Result<Self> {
        if buf.len() < Self::SIZE {
            return Err(io_err(
                io::ErrorKind::InvalidData,
                "universal footer too short",
            ));
        }
        Ok(UniversalFooter {
            magic: u32::from_le_bytes(buf[0..4].try_into().unwrap()),
            num_slices: u32::from_le_bytes(buf[4..8].try_into().unwrap()),
            manifest_offset: u64::from_le_bytes(buf[8..16].try_into().unwrap()),
            manifest_size: u32::from_le_bytes(buf[16..20].try_into().unwrap()),
            reserved: u32::from_le_bytes(buf[20..24].try_into().unwrap()),
        })
    }
}

/// One architecture slice inside the universal binary.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ArchSlice {
    pub target: String,
    pub uname_machine: String,
    pub uname_sys: String,
    pub offset: u64,
    pub size: u64,
    pub sha256: String,
}

/// JSON manifest placed before the universal footer.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct UniversalManifest {
    pub slices: Vec<ArchSlice>,
}

impl UniversalManifest {
    pub fn to_json(&self) -> io::Result<Vec<u8>> {
        serde_json::to_vec_pretty(self).map_err(|e| io_err(io::ErrorKind::Other, &e.to_string()))
    }

    pub fn from_json(buf: &[u8]) -> io::Result<Self> {
        serde_json::from_slice(buf).map_err(|e| io_err(io::ErrorKind::InvalidData, &e.to_string()))
    }
}

/// Generate the polyglot shell-script launcher with hardcoded offsets.
///
/// The script detects the OS/arch and uses `dd` to extract the matching
/// slice to a temp file, then `exec`s it. Offsets are baked in at build
/// time, so no binary parsing is needed at runtime.
pub fn make_launcher(slices: &[ArchSlice]) -> io::Result<Vec<u8>> {
    let mut script = String::from(
        "#!/bin/sh\n# daedalus universal binary — auto-generated launcher\n\
         _arch=$(uname -m)\n_os=$(uname -s 2>/dev/null || echo Linux)\n\
         _self=$0\n_off=\n",
    );

    for s in slices {
        let pattern = format!("{} {}", s.uname_machine, s.uname_sys);
        script.push_str(&format!(
            "case \"$_arch $_os\" in\n  \"{pattern}\") _off={}; _sz={} ;;\nesac\n",
            s.offset, s.size,
        ));
    }

    script.push_str(
        "if [ -z \"$_off\" ]; then\n  echo 'daedalus: unsupported architecture: '\"$_arch\"' on '\"$_os\" >&2\n  exit 1\nfi\n\
        _tmpf=$(mktemp /tmp/daedalus.XXXXXX)\n\
        dd if=\"$_self\" of=\"$_tmpf\" bs=1M skip=$((_off >> 20)) count=$(((_sz + 0xFFFFF) >> 20)) 2>/dev/null || \
        dd if=\"$_self\" of=\"$_tmpf\" bs=1 skip=$_off count=$_sz 2>/dev/null\n\
        chmod +x \"$_tmpf\"\n\
        exec \"$_tmpf\" \"$@\"\n",
    );

    let mut launcher = script.into_bytes();
    launcher.resize(64 * 1_024, 0);
    Ok(launcher)
}

/// Assemble a universal binary from per-arch slices.
///
/// `slices` is a list of (target, `uname_machine`, `uname_sys`, `slice_bytes`).
/// Returns the complete universal binary bytes.
pub fn assemble_universal(slices: &[(&str, &str, &str, &[u8])]) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut arch_slices = Vec::new();

    let launcher_size = 64u64 * 1_024;

    // First pass: collect arch info for the launcher
    for (target, machine, sysname, bytes) in slices {
        let sha256 = hex_sha256(bytes);
        let offset = launcher_size + arch_slices.iter().map(|s: &ArchSlice| s.size).sum::<u64>();
        arch_slices.push(ArchSlice {
            target: target.to_string(),
            uname_machine: machine.to_string(),
            uname_sys: sysname.to_string(),
            offset,
            size: bytes.len() as u64,
            sha256,
        });
    }

    // Generate launcher with hardcoded offsets
    let launcher = make_launcher(&arch_slices)?;
    out.write_all(&launcher)?;

    // Write each slice
    for (_, _, _, bytes) in slices {
        out.write_all(bytes)?;
    }

    // Write manifest (JSON)
    let manifest = UniversalManifest {
        slices: arch_slices.clone(),
    };
    let manifest_bytes = manifest.to_json()?;
    let mut padded_manifest = manifest_bytes.clone();
    padded_manifest.resize(4_096, 0);
    let manifest_offset = out.len() as u64;
    out.write_all(&padded_manifest)?;

    // Write universal footer
    let footer = UniversalFooter {
        magic: UNIV_FOOTER_MAGIC,
        num_slices: manifest.slices.len() as u32,
        manifest_offset,
        manifest_size: manifest_bytes.len() as u32,
        reserved: 0,
    };
    out.write_all(&footer.pack())?;

    Ok(out)
}

/// Assemble a universal binary from pre-built `ArchSlice` metadata and raw
/// slice bytes. This is the entry point used by the CLI when each slice is
/// built separately (e.g. via `cargo zigbuild`).
pub fn assemble_universal_slices(
    slices: &[ArchSlice],
    slice_data: &[Vec<u8>],
) -> io::Result<Vec<u8>> {
    if slices.len() != slice_data.len() {
        return Err(io_err(
            io::ErrorKind::InvalidInput,
            "slice count mismatch: metadata vs data",
        ));
    }

    let mut out = Vec::new();
    let launcher_size = 64u64 * 1_024;

    // Fix up offsets BEFORE generating launcher (launcher bakes in offsets)
    let mut arch_slices = slices.to_vec();
    for (i, bytes) in slice_data.iter().enumerate() {
        arch_slices[i].offset =
            launcher_size + slice_data[..i].iter().map(|b| b.len() as u64).sum::<u64>();
        arch_slices[i].size = bytes.len() as u64;
    }

    let launcher = make_launcher(&arch_slices)?;
    out.write_all(&launcher)?;

    // Write each slice
    for bytes in slice_data {
        out.write_all(bytes)?;
    }

    // Write manifest (JSON)
    let manifest = UniversalManifest {
        slices: arch_slices.clone(),
    };
    let manifest_bytes = manifest.to_json()?;
    let mut padded_manifest = manifest_bytes.clone();
    padded_manifest.resize(4_096, 0);
    let manifest_offset = out.len() as u64;
    out.write_all(&padded_manifest)?;

    // Write universal footer
    let footer = UniversalFooter {
        magic: UNIV_FOOTER_MAGIC,
        num_slices: manifest.slices.len() as u32,
        manifest_offset,
        manifest_size: manifest_bytes.len() as u32,
        reserved: 0,
    };
    out.write_all(&footer.pack())?;

    Ok(out)
}

/// Compute SHA-256 of `data` and return lowercase hex.
pub fn hex_sha256(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash = hasher.finalize();
    let mut s = String::with_capacity(hash.len() * 2);
    for b in hash {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn io_err(kind: io::ErrorKind, msg: &str) -> io::Error {
    io::Error::new(kind, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn universal_footer_roundtrip() {
        let footer = UniversalFooter {
            magic: UNIV_FOOTER_MAGIC,
            num_slices: 2,
            manifest_offset: 12_345,
            manifest_size: 678,
            reserved: 0,
        };
        let packed = footer.pack();
        let parsed = UniversalFooter::parse(&packed).unwrap();
        assert_eq!(parsed.magic, UNIV_FOOTER_MAGIC);
        assert_eq!(parsed.num_slices, 2);
        assert_eq!(parsed.manifest_offset, 12_345);
        assert_eq!(parsed.manifest_size, 678);
    }

    #[test]
    fn assemble_universal_layout() {
        let slices: Vec<(&str, &str, &str, &[u8])> = vec![
            (
                "x86_64-unknown-linux-musl",
                "x86_64",
                "Linux",
                &[0u8; 100][..],
            ),
            (
                "aarch64-unknown-linux-musl",
                "aarch64",
                "Linux",
                &[1u8; 200][..],
            ),
        ];
        let binary = assemble_universal(&slices).unwrap();

        // Launcher is first; first slice starts at page-aligned offset
        assert!(binary.starts_with(b"#!/bin/sh"));

        // Verify the footer is at the end
        let footer =
            UniversalFooter::parse(&binary[binary.len() - UniversalFooter::SIZE..]).unwrap();
        let magic = footer.magic;
        assert_eq!(magic, UNIV_FOOTER_MAGIC);
        assert_eq!(footer.num_slices, 2);

        // Verify manifest
        let manifest_bytes = &binary[footer.manifest_offset as usize
            ..(footer.manifest_offset + u64::from(footer.manifest_size)) as usize];
        let manifest = UniversalManifest::from_json(manifest_bytes).unwrap();
        assert_eq!(manifest.slices.len(), 2);
        assert_eq!(manifest.slices[0].offset, 64 * 1_024);
        assert_eq!(manifest.slices[0].size, 100);
        assert_eq!(manifest.slices[1].offset, 64 * 1_024 + 100);
        assert_eq!(manifest.slices[1].size, 200);
    }

    #[test]
    fn hex_sha256_correct() {
        let hash = hex_sha256(b"hello world");
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn launcher_has_case_for_each_arch() {
        let launcher = make_launcher(&[
            ArchSlice {
                target: "x86_64-unknown-linux-musl".to_string(),
                uname_machine: "x86_64".to_string(),
                uname_sys: "Linux".to_string(),
                offset: 65_536,
                size: 100,
                sha256: String::new(),
            },
            ArchSlice {
                target: "aarch64-unknown-linux-musl".to_string(),
                uname_machine: "aarch64".to_string(),
                uname_sys: "Linux".to_string(),
                offset: 65_636,
                size: 100,
                sha256: String::new(),
            },
        ])
        .unwrap();
        let script = std::str::from_utf8(&launcher).unwrap();
        assert!(script.contains("x86_64 Linux"));
        assert!(script.contains("aarch64 Linux"));
    }
}
