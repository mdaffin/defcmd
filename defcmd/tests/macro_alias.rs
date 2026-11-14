//! End-to-end tests of `#[command(alias = ..)]` through `try_run_with`.
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
    pub fn main() {}

    /// Remove files.
    #[command(alias = rm, alias = "del-all")]
    pub fn remove(#[rest] files: Vec<String>) {
        note(&format!("remove {files:?}"));
    }

    /// List files.
    #[command(alias = "ls-files")]
    pub fn list() {
        note("list");
    }

    /// Sync everything.
    #[command("sync all", alias = everything)]
    pub fn sync_all() {
        note("sync all");
    }

    /// Sync one thing.
    #[command("sync one")]
    pub fn sync_one() {
        note("sync one");
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
fn the_name_and_every_alias_dispatch_to_the_same_command() {
    ok(&["app", "remove", "a"], "remove [\"a\"]");
    ok(&["app", "rm", "b", "c"], "remove [\"b\", \"c\"]");
    ok(&["app", "del-all"], "remove []");
}

#[test]
fn a_string_alias_may_contain_hyphens() {
    ok(&["app", "ls-files"], "list");
    ok(&["app", "list"], "list");
}

#[test]
fn after_double_dash_the_rest_after_an_alias_is_positional() {
    ok(
        &["app", "rm", "--", "-odd-name.txt"],
        "remove [\"-odd-name.txt\"]",
    );
}

#[test]
fn an_alias_replaces_only_the_last_word() {
    ok(&["app", "sync", "everything"], "sync all");
    ok(&["app", "sync", "all"], "sync all");
    let (code, _, err) = run(&["app", "everything"]);
    assert_eq!(code, ExitCode::from(2));
    assert!(
        err.starts_with("Error: unexpected argument 'everything'\n"),
        "{err}"
    );
}

#[test]
fn help_lists_the_aliases_after_the_name() {
    let (code, out, _) = run(&["app", "--help"]);
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(
        out.contains("  app list, ls-files - List files.\n"),
        "{out}"
    );
    assert!(
        out.contains("  app remove, rm, del-all - Remove files.\n"),
        "{out}"
    );
    assert!(
        out.contains("  app sync all, everything - Sync everything.\n"),
        "{out}"
    );
    assert!(out.contains("  app sync one - Sync one thing.\n"), "{out}");
}

#[test]
fn an_alias_gets_the_commands_own_help() {
    let (code, out, _) = run(&["app", "rm", "--help"]);
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(out.starts_with("Remove files.\n"), "{out}");
}
