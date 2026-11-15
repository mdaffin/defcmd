//! Return types, errors and exit codes.
//!
//! ```text
//! $ conf                              # root returns `()`: prints a hint, exit 0
//! $ conf check app.conf               # `Result<(), ConfigError>`: `?` just works
//! $ conf get app.conf port
//! $ conf get app.conf nope            # Error: key `nope` not found in app.conf   (exit 1)
//! $ conf check missing.conf           # Error: cannot read missing.conf: No such file ... (exit 1)
//! $ conf has app.conf port            # `ExitCode`: silent, exit 0 if present, 1 if not
//! $ conf get app.conf                 # usage error, printed by defcmd             (exit 2)
//! $ conf get app.conf port extra      # usage error: unexpected argument 'extra'   (exit 2)
//! ```
//!
//! The last two print, to stderr:
//!
//! ```text
//! Error: missing required argument 'key'
//!   help: Usage: conf get <file> <key>. Use `conf get --help` for more information.
//!
//! Error: unexpected argument 'extra'
//!   help: Usage: conf get <file> <key>. Use `conf get --help` for more information.
//! ```
//!
//! A command may return any of:
//! - `()`                         exit 0
//! - `ExitCode`                   exit with that code, nothing printed
//! - `Result<T, E>`               `T` is any of these (not another `Result`), `E: Display`
//!
//! Unlike nushell, a returned value is not printed: a command prints its own output, as `get`
//! does below.
//!
//! On `Err(e)`, defcmd prints `Error: {e:#}` to stderr and exits 1. `{:#}` means that
//! `anyhow::Error` shows its whole chain, and plain `Display` types are unaffected. This
//! also means `Box<dyn Error>` and `String` work as error types without any glue.
//!
//! Every command converts its result on its own, so different commands in one tree can use
//! different return types, as below.
//!
//! Exit codes used by defcmd itself:
//! - 0  success, and `--help` / `--version` (written to stdout)
//! - 1  the command returned `Err`
//! - 2  usage error (unknown flag, missing value, bad value, ...), written to stderr as
//!      `Error: ...` followed by a `help:` line with the usage and a pointer to `--help`.
//!      Nushell exits 1 here; 2 keeps a usage error apart from `has` answering "no".
//!
//! A command that wants its own codes returns `ExitCode` or `Result<ExitCode, E>`.
//! Nothing in a command body is expected to call `std::process::exit`.

use defcmd::command;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug)]
enum ConfigError {
    Read { path: PathBuf, source: io::Error },
    MissingKey { path: PathBuf, key: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(f, "cannot read {}: {source}", path.display())
            }
            Self::MissingKey { path, key } => {
                write!(f, "key `{key}` not found in {}", path.display())
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// Read `key = value` lines, ignoring blanks and `#` comments.
fn load(path: &Path) -> Result<Vec<(String, String)>, ConfigError> {
    let text = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect())
}

/// Inspect simple `key = value` config files.
#[command]
fn main() {
    // Runs only when no subcommand is given; `()` means exit 0.
    println!("conf: try `conf --help`");
}

/// Check that a config file can be read.
#[command]
fn check(
    /// Config file.
    file: PathBuf,
) -> Result<(), ConfigError> {
    let entries = load(&file)?;
    println!("{}: {} entries", file.display(), entries.len());
    Ok(())
}

/// Print the value of a key.
#[command]
fn get(
    /// Config file.
    file: PathBuf,
    /// Key to look up.
    key: String,
) -> Result<(), ConfigError> {
    let entries = load(&file)?;
    let (_, value) = entries
        .into_iter()
        .find(|(k, _)| *k == key)
        .ok_or_else(|| ConfigError::MissingKey {
            path: file.clone(),
            key: key.clone(),
        })?;
    println!("{value}");
    Ok(())
}

/// Exit 0 if the key is present, 1 if not. Prints nothing (like `grep -q`).
#[command]
fn has(
    /// Config file.
    file: PathBuf,
    /// Key to look up.
    key: String,
) -> Result<ExitCode, ConfigError> {
    // `?` still reports an unreadable file as an error (exit 1 with a message), while the
    // "not found" answer is a plain exit code with no message.
    let entries = load(&file)?;
    Ok(if entries.iter().any(|(k, _)| *k == key) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test checks the whole command tree (duplicate names and so on).
    #[test]
    fn tree_is_valid() {
        defcmd::check();
    }

    // Every command except `main` is still an ordinary function, so call it directly.
    #[test]
    fn unreadable_file_is_an_error() {
        let err = check(PathBuf::from("/nonexistent/app.conf")).unwrap_err();
        assert!(
            err.to_string()
                .starts_with("cannot read /nonexistent/app.conf")
        );
    }

    // `try_run_with` runs the real parse-and-dispatch path and captures defcmd's own output
    // (help, usage errors and `Error:` lines). A body's `println!` is not captured.
    #[test]
    fn extra_argument_is_a_usage_error() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = defcmd::try_run_with(
            ["conf", "get", "a.conf", "port", "extra"],
            &mut out,
            &mut err,
        );
        assert_eq!(code, ExitCode::from(2));
        let err = String::from_utf8(err).unwrap();
        assert!(err.starts_with("Error: unexpected argument 'extra'"));
    }
}
