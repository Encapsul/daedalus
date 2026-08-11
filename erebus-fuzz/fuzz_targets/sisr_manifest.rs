//! Fuzz targets for the `SISR` binary parsers.
//!
//! Requires a nightly toolchain + `cargo-fuzz` (`cargo install cargo-fuzz`),
//! so this is excluded from the stable CI suite. Run:
//!
//! ```bash
//! cargo +nightly fuzz run sisr_manifest
//! ```

#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;

use xbin_core::format::Footer;
use xbin_core::manifest::DeltaManifest;
use xbin_core::sisr_header::{read_sisr, SisrFooterExt};
use xbin_core::sisr_stage::RemoteManifest;

fuzz_target!(|data: &[u8]| {
    let _ = DeltaManifest::parse(data);
    let _ = SisrFooterExt::parse(data);
    let _ = RemoteManifest::from_bytes(data);
    let _ = Footer::read_from(&mut Cursor::new(data));
    let _ = read_sisr(&mut Cursor::new(data));
});
