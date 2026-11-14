//! End-to-end tests of bare `#[command]` subcommands through `try_run_with`.
//!
//! One root per process (the registry is global), so every test here uses this one tree.

use std::process::ExitCode;
use std::sync::Mutex;

static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn note(msg: impl Into<String>) {
    SEEN.lock().unwrap().push(msg.into());
}

fn seen(msg: &str) -> bool {
    SEEN.lock().unwrap().iter().any(|m| m == msg)
}

mod app {
    use super::note;
    use defcmd::command;
    use std::path::PathBuf;
    use std::process::ExitCode;

    /// Inspect things.
    #[command]
    pub fn main() {
        note("root");
    }

    /// Check a file.
    ///
    /// Reads it and counts.
    #[command]
    pub fn check(
        /// Config file.
        file: PathBuf,
    ) -> Result<(), String> {
        note(format!("check {}", file.display()));
        if file.as_os_str() == "bad" {
            Err("cannot read bad".to_string())
        } else {
            Ok(())
        }
    }

    #[command]
    pub fn list_all(
        /// Print more.
        #[flag]
        long: bool,
    ) {
        note(format!("list-all long={long}"));
    }

    #[command]
    pub fn r#match(key: String) -> ExitCode {
        note(format!("match {key}"));
        ExitCode::from(if key == "yes" { 0 } else { 3 })
    }

    // A parameter named like the function does not hide the function from the glue.
    #[command]
    pub fn echo(echo: String) {
        note(format!("echo {echo}"));
    }
}

fn run(args: &[&str]) -> (ExitCode, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = defcmd::try_run_with(args.iter().copied(), &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn the_root_runs_when_no_subcommand_is_given() {
    let (code, out, err) = run(&["app"]);
    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!((out.as_str(), err.as_str()), ("", ""));
    assert!(seen("root"));
}

#[test]
fn a_subcommand_runs_with_its_arguments() {
    let (code, out, err) = run(&["app", "check", "a.conf"]);
    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!((out.as_str(), err.as_str()), ("", ""));
    assert!(seen("check a.conf"));
}

#[test]
fn an_err_result_is_reported_with_exit_1() {
    let (code, out, err) = run(&["app", "check", "bad"]);
    assert_eq!(code, ExitCode::from(1));
    assert_eq!(out, "");
    assert_eq!(err, "Error: cannot read bad\n");
}

#[test]
fn names_come_from_the_function_with_underscores_as_dashes() {
    let (code, ..) = run(&["app", "list-all", "--long"]);
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(seen("list-all long=true"));
    // The Rust spelling is not a command.
    let (code, _, err) = run(&["app", "list_all"]);
    assert_eq!(code, ExitCode::from(2));
    assert!(
        err.starts_with("Error: unexpected argument 'list_all'\n"),
        "{err}"
    );
}

#[test]
fn raw_identifiers_lose_the_prefix() {
    let (code, ..) = run(&["app", "match", "yes"]);
    assert_eq!(code, ExitCode::SUCCESS);
    let (code, out, err) = run(&["app", "match", "no"]);
    assert_eq!(code, ExitCode::from(3));
    assert_eq!((out.as_str(), err.as_str()), ("", ""));
    assert!(seen("match no"));
}

#[test]
fn a_parameter_named_like_its_function_works() {
    let (code, ..) = run(&["app", "echo", "hi"]);
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(seen("echo hi"));
}

#[test]
fn root_help_lists_every_subcommand_sorted() {
    let (code, out, err) = run(&["app", "--help"]);
    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!(err, "");
    let expected = "\
Inspect things.

Usage:
  > app

Subcommands:
  app check - Check a file.
  app echo
  app list-all
  app match

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version
";
    assert_eq!(out, expected);
}

#[test]
fn subcommand_help_and_usage_errors_use_the_full_path() {
    let (code, out, _) = run(&["app", "check", "--help"]);
    assert_eq!(code, ExitCode::SUCCESS);
    let expected = "\
Check a file.

Reads it and counts.

Usage:
  > app check <file>

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version

Parameters:
  file <path>: Config file.
";
    assert_eq!(out, expected);

    let (code, _, err) = run(&["app", "check"]);
    assert_eq!(code, ExitCode::from(2));
    assert_eq!(
        err,
        "Error: missing required argument 'file'\n  \
         help: Usage: app check <file>. Use `app check --help` for more information.\n"
    );
}

#[test]
fn subcommands_stay_ordinary_functions() {
    assert_eq!(app::check("bad".into()), Err("cannot read bad".to_string()));
    assert_eq!(app::check("ok".into()), Ok(()));
    app::list_all(false);
    assert!(seen("list-all long=false"));
}
