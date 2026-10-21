use std::io::ErrorKind;
use std::process::ExitCode;

/// Exit codes per clig.dev conventions.
///
/// - `0`: Success
/// - `1`: General error
/// - `2`: Usage / lint / verification error
/// - `3`: Data error (corrupt footer/metadata, invalid payload, format parse)
/// - `4`: Permission error
/// - `5`: Not found
pub fn exit_code_for_error(err: &anyhow::Error) -> ExitCode {
    ExitCode::from(classify_error(err))
}

/// Numeric exit status for an error (same mapping as [`exit_code_for_error`]).
fn classify_error(err: &anyhow::Error) -> u8 {
    for cause in err.chain() {
        if let Some(io_err) = cause.downcast_ref::<std::io::Error>() {
            return match io_err.kind() {
                ErrorKind::NotFound => 5,
                ErrorKind::PermissionDenied => 4,
                ErrorKind::InvalidData | ErrorKind::InvalidInput | ErrorKind::UnexpectedEof => 3,
                _ => 1,
            };
        }
    }

    let msg = format!("{err}").to_lowercase();
    if msg.contains("not found") || msg.contains("not a file") || msg.contains("not a directory") {
        return 5;
    }
    if msg.contains("permission denied") || msg.contains("not writable") {
        return 4;
    }
    // Verification / lint before data heuristics so signature wording stays exit 2.
    if msg.contains("already signed") || msg.contains("signature") || msg.contains("lint") {
        return 2;
    }
    if is_data_error(&msg) {
        return 3;
    }

    1
}

/// True when the error message describes a corrupt or unparseable artifact.
fn is_data_error(msg: &str) -> bool {
    const MARKERS: &[&str] = &[
        "bad magic",
        "bad footer",
        "truncated",
        "invalid format",
        "invalid payload",
        "invalid .de",
        "invalid .daedalus",
        "not a valid .de",
        "not a valid daedalus",
        "not a .de",
        "not a .daedalus",
        "not a packaged artifact",
        "failed to read daedalus footer",
        "failed to read footer",
        "failed to parse metadata",
        "corrupt",
        "malformed",
    ];
    MARKERS.iter().any(|m| msg.contains(m))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn io_invalid_data_is_exit_3() {
        let err = anyhow::Error::new(io::Error::new(ErrorKind::InvalidData, "bad magic"));
        assert_eq!(classify_error(&err), 3);
    }

    #[test]
    fn io_not_found_is_exit_5() {
        let err = anyhow::Error::new(io::Error::new(ErrorKind::NotFound, "missing"));
        assert_eq!(classify_error(&err), 5);
    }

    #[test]
    fn signature_mismatch_is_exit_2() {
        assert_eq!(
            classify_error(&anyhow::anyhow!("signature verification failed")),
            2
        );
    }

    #[test]
    fn invalid_de_message_is_exit_3() {
        assert_eq!(
            classify_error(&anyhow::anyhow!("stdin is not a valid .de file")),
            3
        );
        assert_eq!(
            classify_error(&anyhow::anyhow!("failed to read daedalus footer")),
            3
        );
    }

    #[test]
    fn wrapped_footer_io_error_is_exit_3() {
        let err = anyhow::Error::new(io::Error::new(
            ErrorKind::InvalidData,
            "bad footer sentinel",
        ))
        .context("failed to read daedalus footer");
        assert_eq!(classify_error(&err), 3);
    }
}
