//! What a command may return. Only `()` for now.

use std::io::Write;
use std::process::ExitCode;

/// A command's return value, turned into an exit code. Anything that needs reporting (an `Err`)
/// is written to `err`.
pub trait Output {
    fn report(self, err: &mut dyn Write) -> ExitCode;
}

impl Output for () {
    fn report(self, _err: &mut dyn Write) -> ExitCode {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_is_success_and_prints_nothing() {
        let mut err = Vec::new();
        assert_eq!(().report(&mut err), ExitCode::SUCCESS);
        assert!(err.is_empty());
    }
}
