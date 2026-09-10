//! stdin/stdout plumbing for `-` paths.
//!
//! Every command that takes a `.de` binary (run, inspect, sign, verify, swap)
//! or produces one (`build -o`, `sign`) accepts `-` to mean "standard stream".
//! A binary is read from stdin once and served as a seekable `Cursor`, or an
//! artifact is written to stdout via a seekable temp file (assembly and the
//! integrity hash both need random access, so a raw stdout pipe is not enough
//! — bytes are staged in a temp file inside the cache dir, then streamed out).
//!
//! Reading stdin is capped so a hostile upstream pipe cannot OOM the process.

use anyhow::{Context, Result};
use std::io::{Cursor, IsTerminal, Read, Write};
use std::path::Path;

/// Maximum bytes accepted from stdin for a `-` input (512 MiB).
const MAX_STDIN_BYTES: u64 = 512 << 20;

/// Whether a CLI string is the stdin/stdout sentinel `-`.
pub fn is_dash(s: &str) -> bool {
    s == "-"
}

/// Read standard input fully into a `Cursor` so callers get a `Read + Seek`
/// source (the same contract as an opened file).
pub fn read_stdin_cursor() -> Result<Cursor<Vec<u8>>> {
    Ok(Cursor::new(read_stdin()?))
}

/// Read standard input into a byte buffer, bounded by [`MAX_STDIN_BYTES`].
fn read_stdin() -> Result<Vec<u8>> {
    if std::io::stdin().is_terminal() {
        anyhow::bail!(
            "stdin is an interactive terminal but this command expects piped input; \
             pipe a .de file in (e.g. `cat app.de | daedalus inspect -`) or pass a file path"
        );
    }
    let mut total: u64 = 0;
    let mut buf = Vec::new();
    let mut stdin = std::io::stdin().lock();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let n = stdin.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_STDIN_BYTES {
            anyhow::bail!("stdin exceeds {MAX_STDIN_BYTES} bytes");
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    Ok(buf)
}

/// Fix up a relative temp path whose basename matters (`app.de`).
///
/// `assemble_daedalus` derives the output extension from the path, so a temp
/// path must keep the `.de` suffix the user asked for. The temp file also
/// needs to sit somewhere writable on every host the run on which it stages.
pub fn temp_sink_from_output(output: &Path) -> Result<std::path::PathBuf> {
    let dir = daedalus_core::paths::cache_dir().join("builds");
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    let name = output
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "app.de".to_string());
    let tmp = dir.join(format!(
        "{name}.{}.tmp",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    Ok(tmp)
}

/// Stream the contents of `src` to stdout in bounded chunks.
pub fn copy_to_stdout(src: &Path) -> Result<()> {
    let mut file = std::fs::File::open(src)?;
    let mut stdout = std::io::stdout().lock();
    std::io::copy(&mut file, &mut stdout)?;
    stdout.flush()?;
    Ok(())
}

/// Whether a build/output path means "standard output" (`-`).
pub fn is_stdout_output(p: &Path) -> bool {
    p.to_string_lossy() == "-"
}
