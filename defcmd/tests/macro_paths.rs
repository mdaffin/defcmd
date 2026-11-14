//! End-to-end tests of string-named commands (`#[command("sync all")]`) through `try_run_with`.
//!
//! One root per process (the registry is global), so every test here uses this one tree.

use std::process::ExitCode;
use std::sync::Mutex;

static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn note(msg: &str) {
    SEEN.lock().unwrap().push(msg.to_string());
}

fn seen(msg: &str) -> bool {
    SEEN.lock().unwrap().iter().any(|m| m == msg)
}

mod app {
    use super::note;
    use defcmd::command;

    /// The root.
    #[command]
    pub fn main() {
        note("root");
    }

    /// Sync things.
    #[command("sync")]
    pub fn whatever_this_is_called() {
        note("sync");
    }

    /// Sync everything.
    #[command("sync all")]
    pub fn x(name: String) {
        note(&format!("sync all {name}"));
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
fn a_string_path_replaces_the_function_name() {
    let (code, _, err) = run(&["app", "sync"]);
    assert_eq!((code, err.as_str()), (ExitCode::SUCCESS, ""));
    assert!(seen("sync"));
    let (code, _, err) = run(&["app", "whatever-this-is-called"]);
    assert_eq!(code, ExitCode::from(2));
    assert!(
        err.contains("unexpected argument 'whatever-this-is-called'"),
        "{err}"
    );
}

#[test]
fn a_path_of_several_words_nests() {
    let (code, _, err) = run(&["app", "sync", "all"]);
    // `sync` takes no positional, so `all` is a subcommand of it; `all` needs its argument.
    assert_eq!(code, ExitCode::from(2));
    assert!(
        err.starts_with("Error: missing required argument 'name'"),
        "{err}"
    );
    let (code, _, err) = run(&["app", "sync", "all", "origin"]);
    assert_eq!((code, err.as_str()), (ExitCode::SUCCESS, ""));
    assert!(seen("sync all origin"));
}

#[test]
fn help_shows_the_full_path() {
    let (code, out, _) = run(&["app", "--help"]);
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(
        out.contains("  app sync - Sync things.\n  app sync all - Sync everything.\n"),
        "{out}"
    );
}
