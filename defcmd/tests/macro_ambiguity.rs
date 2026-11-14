//! A root that takes positionals and also has a subcommand: the subcommand word wins, and `--`
//! passes the same word as a positional (see `examples/flags_and_arguments.rs`).
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

    /// Copy a file.
    #[command]
    pub fn main(
        /// Source file.
        from: String,
        /// Destination.
        #[default(".")]
        to: String,
        #[flag] force: bool,
    ) {
        note(&format!("copy {from} {to} force={force}"));
    }

    /// Remove files.
    #[command(alias = rm)]
    pub fn remove(#[rest] files: Vec<String>) {
        note(&format!("remove {files:?}"));
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

#[test]
fn the_subcommand_word_wins_over_the_first_positional() {
    ok(&["copy", "remove"], "remove []");
    ok(&["copy", "remove", "a", "b"], "remove [\"a\", \"b\"]");
}

#[test]
fn an_alias_is_a_subcommand_word_too() {
    ok(&["copy", "rm", "x"], "remove [\"x\"]");
}

#[test]
fn double_dash_makes_the_word_a_positional() {
    ok(&["copy", "--", "remove"], "copy remove . force=false");
    ok(&["copy", "--", "rm", "dest"], "copy rm dest force=false");
}

#[test]
fn flags_before_the_word_do_not_stop_it_being_a_subcommand() {
    // The flag belongs to the root and is only validated; the subcommand runs.
    ok(&["copy", "--force", "remove", "z"], "remove [\"z\"]");
}

#[test]
fn another_word_is_the_first_positional() {
    ok(&["copy", "a.txt"], "copy a.txt . force=false");
    ok(
        &["copy", "a.txt", "remove"],
        "copy a.txt remove force=false",
    );
}

#[test]
fn only_the_first_positional_can_be_a_subcommand() {
    // `remove` as the second word is the destination, not a subcommand.
    ok(
        &["copy", "a", "remove", "--force"],
        "copy a remove force=true",
    );
}

#[test]
fn the_root_help_lists_the_subcommand_and_both_positionals() {
    let (code, out, _) = run(&["copy", "--help"]);
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(out.contains("  > copy {flags} <from> (to)\n"), "{out}");
    assert!(out.contains("  copy remove, rm - Remove files.\n"), "{out}");
}
