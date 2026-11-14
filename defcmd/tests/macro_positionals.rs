//! End-to-end tests of optional and default positionals through `try_run_with`.
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

    /// Copy files.
    #[command]
    pub fn main() {}

    /// Copy a file.
    #[command]
    pub fn copy(
        /// Source file.
        from: PathBuf,
        /// Destination; the current directory if omitted.
        #[default(".")]
        to: PathBuf,
        /// A note.
        label: Option<String>,
        #[flag] force: bool,
    ) {
        note(format!(
            "copy {} {} {label:?} force={force}",
            from.display(),
            to.display()
        ));
    }

    /// Count up.
    #[command]
    pub fn count(#[default(3)] times: u32) {
        note(format!("count {times}"));
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

fn ok(args: &[&str], expected: &str) {
    let (code, out, err) = run(args);
    assert_eq!(code, ExitCode::SUCCESS, "{args:?}: {err}");
    assert_eq!((out.as_str(), err.as_str()), ("", ""));
    assert!(
        seen(expected),
        "{expected:?} not in {:?}",
        SEEN.lock().unwrap()
    );
}

fn usage_error(args: &[&str], message: &str) {
    let (code, out, err) = run(args);
    assert_eq!(code, ExitCode::from(2), "{args:?}");
    assert_eq!(out, "");
    assert!(
        err.starts_with(&format!("Error: {message}\n")),
        "{args:?}: {err}"
    );
}

#[test]
fn absent_optional_positionals_are_none_or_their_default() {
    ok(&["app", "copy", "a.txt"], "copy a.txt . None force=false");
}

#[test]
fn positionals_fill_in_declaration_order_with_flags_anywhere() {
    ok(
        &["app", "copy", "a.txt", "dir", "note"],
        "copy a.txt dir Some(\"note\") force=false",
    );
    ok(
        &[
            "app",
            "copy",
            "--force",
            "b.txt",
            "dir2",
            "--force=true",
            "n2",
        ],
        "copy b.txt dir2 Some(\"n2\") force=true",
    );
}

#[test]
fn a_default_of_another_type_needs_no_suffix() {
    ok(&["app", "count"], "count 3");
    ok(&["app", "count", "7"], "count 7");
}

#[test]
fn a_bad_value_for_a_default_positional_is_reported() {
    usage_error(
        &["app", "count", "x"],
        "invalid value 'x' for 'times': expected int",
    );
}

#[test]
fn the_required_positional_is_still_required() {
    usage_error(&["app", "copy"], "missing required argument 'from'");
}

#[test]
fn extra_positionals_are_unexpected() {
    usage_error(
        &["app", "copy", "a", "b", "c", "d"],
        "unexpected argument 'd'",
    );
}

#[test]
fn help_marks_optional_parameters() {
    let (code, out, _) = run(&["app", "copy", "--help"]);
    assert_eq!(code, ExitCode::SUCCESS);
    let expected = "\
Copy a file.

Usage:
  > app copy {flags} <from> (to) (label)

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version
  --force

Parameters:
  from <path>: Source file.
  to <path>: Destination; the current directory if omitted. (optional, default: '.')
  label <string>: A note. (optional)
";
    assert_eq!(out, expected);
}
