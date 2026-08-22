use std::process::ExitCode;

/// Exit codes per clig.dev conventions.
///
/// - `0`: Success
/// - `1`: General error
/// - `2`: Usage / lint / verification error
/// - `3`: Data error
/// - `4`: Permission error
/// - `5`: Not found
pub fn exit_code_for_error(err: &anyhow::Error) -> ExitCode {
    for cause in err.chain() {
        if let Some(io_err) = cause.downcast_ref::<std::io::Error>() {
            return match io_err.kind() {
                std::io::ErrorKind::NotFound => ExitCode::from(5),
                std::io::ErrorKind::PermissionDenied => ExitCode::from(4),
                _ => ExitCode::from(1),
            };
        }
    }

    let msg = format!("{err}");
    if msg.contains("not found") || msg.contains("not a file") || msg.contains("not a directory") {
        return ExitCode::from(5);
    }
    if msg.contains("permission denied") || msg.contains("not writable") {
        return ExitCode::from(4);
    }
    if msg.contains("already signed") || msg.contains("signature") || msg.contains("lint") {
        return ExitCode::from(2);
    }

    ExitCode::from(1)
}
