//! End-to-end: `#[flag(global)]` on the root's flags, repeated on the commands that want them.
//!
//! One root per process (the registry is global), so every test here uses this one tree.

use std::process::ExitCode;
use std::sync::Mutex;

/// What each body saw, one line per call.
static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn seen(line: String) {
    SEEN.lock().unwrap().push(line);
}

fn was_seen(line: &str) -> bool {
    SEEN.lock().unwrap().iter().any(|l| l == line)
}

mod app {
    use defcmd::command;

    /// The root.
    #[command]
    pub fn main(
        /// Chatty output.
        #[flag(global, -v)]
        verbose: bool,
        /// How many jobs.
        #[flag(global)]
        jobs: Option<u8>,
        /// Tags to apply.
        #[flag(global)]
        tag: Vec<String>,
        /// Only for the root.
        #[flag]
        local: bool,
    ) {
        super::seen(format!("root {verbose} {jobs:?} {tag:?} {local}"));
    }

    /// List things.
    #[command]
    pub fn list(
        /// What to list.
        name: String,
        /// Chatty output.
        #[flag(global, -v)]
        verbose: bool,
        /// Tags to apply.
        #[flag(global)]
        tag: Vec<String>,
    ) {
        super::seen(format!("list {name} {verbose} {tag:?}"));
    }

    /// Takes no globals.
    #[command]
    pub fn plain(name: String) {
        super::seen(format!("plain {name}"));
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
fn the_tree_is_valid() {
    defcmd::check();
}

#[test]
fn the_root_gets_its_global_flags() {
    let (code, ..) = run(&["app", "-v", "--jobs", "4", "--tag", "a", "--local"]);
    assert!(code == ExitCode::SUCCESS);
    assert!(was_seen("root true Some(4) [\"a\"] true"));
}

#[test]
fn a_command_gets_the_global_flag_from_either_side_of_its_name() {
    for (args, name) in [
        (&["app", "-v", "list", "before"][..], "before"),
        (&["app", "list", "after", "-v"], "after"),
        (&["app", "list", "-v", "between"], "between"),
    ] {
        let (code, _, err) = run(args);
        assert!(code == ExitCode::SUCCESS, "{args:?}: {err}");
        assert!(was_seen(&format!("list {name} true []")), "{args:?}");
    }
}

#[test]
fn a_repeatable_global_flag_collects_values_from_both_levels() {
    let (code, ..) = run(&["app", "--tag", "x", "list", "joined", "--tag", "y"]);
    assert!(code == ExitCode::SUCCESS);
    assert!(was_seen("list joined false [\"x\", \"y\"]"));
}

#[test]
fn a_command_that_does_not_declare_a_global_flag_still_accepts_it() {
    let (code, _, err) = run(&["app", "--jobs", "2", "plain", "quiet", "-v"]);
    assert!(code == ExitCode::SUCCESS, "{err}");
    assert!(was_seen("plain quiet"));
}

#[test]
fn a_root_flag_that_is_not_global_cannot_come_before_a_command() {
    let (code, _, err) = run(&["app", "--local", "list", "refused"]);
    assert!(code == ExitCode::from(2));
    assert!(
        err.starts_with("Error: flag '--local' has no effect before the subcommand 'list'\n"),
        "{err}"
    );
    assert!(!was_seen("list refused false []"));
}

#[test]
fn a_global_flag_is_not_an_unknown_flag_of_the_command() {
    let (code, _, err) = run(&["app", "list", "x", "--jobs", "nope"]);
    // `list` does not declare `--jobs`, so the value is not converted by it.
    assert!(code == ExitCode::SUCCESS, "{err}");
    assert!(was_seen("list x false []"));
}

#[test]
fn a_commands_help_lists_the_globals_it_declares_and_those_it_only_accepts() {
    let (code, out, _) = run(&["app", "plain", "--help"]);
    assert!(code == ExitCode::SUCCESS);
    assert!(
        out.contains("Usage:\n  > app plain {flags} <name>\n"),
        "{out}"
    );
    for flag in [
        "-v, --verbose",
        "--jobs <int>",
        "--tag <string>: Tags to apply. (repeatable)",
    ] {
        assert!(out.contains(flag), "{flag}: {out}");
    }
    assert!(!out.contains("--local"), "{out}");

    // `list` declares `--verbose` and `--tag` itself: each is listed once.
    let (_, out, _) = run(&["app", "list", "--help"]);
    assert_eq!(out.matches("--verbose").count(), 1, "{out}");
    assert_eq!(out.matches("--tag").count(), 1, "{out}");
    assert_eq!(out.matches("--jobs").count(), 1, "{out}");
}
