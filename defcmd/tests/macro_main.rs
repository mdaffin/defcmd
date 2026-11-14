//! End-to-end tests of `#[command] fn main` through `try_run_with`.
//!
//! The root lives in a module so the test harness keeps its own `main`. There is one root per
//! process (the registry is global), so every test here uses this one command.

use std::process::ExitCode;
use std::sync::Mutex;

static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

mod app {
    use super::SEEN;
    use defcmd::command;

    /// Print a greeting.
    #[command]
    pub fn main(
        /// Who to greet.
        name: String,
        /// Print in upper case.
        #[flag]
        shout: bool,
    ) {
        let msg = format!("Hello, {name}!");
        let msg = if shout { msg.to_uppercase() } else { msg };
        SEEN.lock().unwrap().push(msg);
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

fn seen(msg: &str) -> bool {
    SEEN.lock().unwrap().iter().any(|m| m == msg)
}

#[test]
fn runs_the_body_with_the_arguments() {
    let (code, out, err) = run(&["hello", "Ada"]);
    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!((out.as_str(), err.as_str()), ("", ""));
    assert!(seen("Hello, Ada!"));
}

#[test]
fn switches_may_come_before_or_after_positionals() {
    assert_eq!(run(&["hello", "--shout", "Bob"]).0, ExitCode::SUCCESS);
    assert!(seen("HELLO, BOB!"));
    assert_eq!(run(&["hello", "Cy", "--shout"]).0, ExitCode::SUCCESS);
    assert!(seen("HELLO, CY!"));
    assert_eq!(run(&["hello", "Di", "--shout=false"]).0, ExitCode::SUCCESS);
    assert!(seen("Hello, Di!"));
}

#[test]
fn help_comes_from_the_doc_comments() {
    let (code, out, err) = run(&["hello", "--help"]);
    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!(err, "");
    assert_eq!(
        out,
        "\
Print a greeting.

Usage:
  > hello {flags} <name>

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version
  --shout: Print in upper case.

Parameters:
  name <string>: Who to greet.
"
    );
}

#[test]
fn version_is_the_crate_version() {
    let (code, out, _) = run(&["hello", "--version"]);
    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!(out, format!("hello {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn missing_positional_is_a_usage_error() {
    let (code, out, err) = run(&["hello"]);
    assert_eq!(code, ExitCode::from(2));
    assert_eq!(out, "");
    assert_eq!(
        err,
        "Error: missing required argument 'name'\n  \
         help: Usage: hello {flags} <name>. Use `hello --help` for more information.\n"
    );
}

#[test]
fn unknown_flag_is_a_usage_error() {
    let (code, _, err) = run(&["hello", "Ada", "--nope"]);
    assert_eq!(code, ExitCode::from(2));
    assert!(err.starts_with("Error: unknown flag '--nope'\n"));
}
