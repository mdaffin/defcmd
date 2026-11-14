//! What a command may return: `()`, `ExitCode`, and `Result` of either.

use std::fmt::Display;
use std::io::Write;
use std::process::ExitCode;

/// A command's return value, turned into an exit code. Anything that needs reporting (an `Err`)
/// is written to `err`.
pub trait Output {
    fn report(self, err: &mut dyn Write) -> ExitCode;
}

/// The success types a `Result` may carry: `()` and `ExitCode`. Keeps `Result<Result<..>>` from
/// being an `Output`.
pub trait Success {
    fn exit_code(self) -> ExitCode;
}

impl Success for () {
    fn exit_code(self) -> ExitCode {
        ExitCode::SUCCESS
    }
}

impl Success for ExitCode {
    fn exit_code(self) -> ExitCode {
        self
    }
}

impl Output for () {
    fn report(self, _err: &mut dyn Write) -> ExitCode {
        ExitCode::SUCCESS
    }
}

impl Output for ExitCode {
    fn report(self, _err: &mut dyn Write) -> ExitCode {
        self
    }
}

impl<T: Success, E: Display> Output for Result<T, E> {
    fn report(self, err: &mut dyn Write) -> ExitCode {
        match self {
            Ok(value) => value.exit_code(),
            Err(e) => {
                // Ignore write errors (closed pipe).
                let _ = writeln!(err, "Error: {e:#}");
                ExitCode::FAILURE
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt;

    #[test]
    fn unit_is_success_and_prints_nothing() {
        let mut err = Vec::new();
        assert_eq!(().report(&mut err), ExitCode::SUCCESS);
        assert!(err.is_empty());
    }

    #[test]
    fn exit_code_passes_through() {
        let mut err = Vec::new();
        assert_eq!(ExitCode::from(7).report(&mut err), ExitCode::from(7));
        assert!(err.is_empty());
    }

    #[test]
    fn ok_unit_and_ok_exit_code() {
        let mut err = Vec::new();
        assert_eq!(Ok::<(), String>(()).report(&mut err), ExitCode::SUCCESS);
        assert_eq!(
            Ok::<ExitCode, String>(ExitCode::from(3)).report(&mut err),
            ExitCode::from(3)
        );
        assert!(err.is_empty());
    }

    fn stderr_of(result: impl Output) -> (ExitCode, String) {
        let mut err = Vec::new();
        let code = result.report(&mut err);
        (code, String::from_utf8(err).unwrap())
    }

    #[test]
    fn err_string_prints_error_line_and_fails() {
        let (code, text) = stderr_of(Err::<(), _>(String::from("boom")));
        assert_eq!(code, ExitCode::FAILURE);
        assert_eq!(text, "Error: boom\n");
    }

    #[test]
    fn err_boxed_error() {
        let boxed: Box<dyn std::error::Error> = "bad thing".into();
        let (code, text) = stderr_of(Err::<ExitCode, _>(boxed));
        assert_eq!(code, ExitCode::FAILURE);
        assert_eq!(text, "Error: bad thing\n");
    }

    #[test]
    fn err_custom_error_uses_alternate_display() {
        struct Custom;
        impl fmt::Display for Custom {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                if f.alternate() {
                    write!(f, "outer: inner")
                } else {
                    write!(f, "outer")
                }
            }
        }
        let (code, text) = stderr_of(Err::<(), _>(Custom));
        assert_eq!(code, ExitCode::FAILURE);
        assert_eq!(text, "Error: outer: inner\n");
    }
}
