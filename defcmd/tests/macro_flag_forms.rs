//! End-to-end tests of `Option`, `Vec` and tri-state flags, negation and bundling through
//! `try_run_with`.
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

    /// Copy things.
    #[command]
    pub fn main(
        to: Option<String>,
        /// Retry count.
        #[flag]
        retries: Option<u32>,
        /// Skip patterns.
        #[flag(-x)]
        exclude: Vec<String>,
        /// Include dirs.
        #[flag(--include, -I)]
        dirs: Vec<std::path::PathBuf>,
        /// Name.
        #[flag]
        name: Option<String>,
    ) {
        note(format!(
            "to={to:?} retries={retries:?} exclude={exclude:?} dirs={dirs:?} name={name:?}"
        ));
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
fn absent_option_and_vec_flags_are_none_and_empty() {
    ok(
        &["app"],
        "to=None retries=None exclude=[] dirs=[] name=None",
    );
}

#[test]
fn option_flag_holds_its_value() {
    ok(
        &["app", "--retries", "5", "--name=-x"],
        "to=None retries=Some(5) exclude=[] dirs=[] name=Some(\"-x\")",
    );
}

#[test]
fn vec_flag_collects_every_occurrence_in_order() {
    ok(
        &[
            "app",
            "-x",
            "*.tmp",
            "--exclude",
            "*.bak",
            "-I",
            "a",
            "--include=b",
        ],
        "to=None retries=None exclude=[\"*.tmp\", \"*.bak\"] dirs=[\"a\", \"b\"] name=None",
    );
}

#[test]
fn vec_flag_takes_one_value_per_occurrence() {
    ok(
        &["app", "-x", "p", "q"],
        "to=Some(\"q\") retries=None exclude=[\"p\"] dirs=[] name=None",
    );
}

#[test]
fn bad_values_name_the_flag() {
    usage_error(
        &["app", "--retries", "x"],
        "invalid value 'x' for '--retries': expected int",
    );
}

#[test]
fn generated_help_uses_the_inner_type_and_marks_repeatable_flags() {
    let (code, out, _) = run(&["app", "--help"]);
    assert_eq!(code, ExitCode::SUCCESS);
    let expected = "\
Copy things.

Usage:
  > app {flags} (to)

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version
  --retries <int>: Retry count.
  -x, --exclude <string>: Skip patterns. (repeatable)
  -I, --include <path>: Include dirs. (repeatable)
  --name <string>: Name.

Parameters:
  to <string> (optional)
";
    assert_eq!(out, expected);
}
