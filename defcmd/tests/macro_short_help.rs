//! End-to-end: a user `-h` or `-V` replaces the generated short form.
//!
//! One root per process (the registry is global), so every test here uses this one tree.

use std::process::ExitCode;

mod app {
    use defcmd::command;

    /// The root.
    #[command]
    pub fn main(
        /// The host to use.
        #[flag(-h)]
        host: Option<String>,
        /// Chatty output.
        #[flag(-V)]
        verbose: bool,
    ) {
        let _ = (host, verbose);
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
fn help_lists_the_long_forms_only_when_the_short_ones_are_taken() {
    let (code, out, _) = run(&["app", "--help"]);
    assert!(code == ExitCode::SUCCESS);
    assert!(
        out.contains("\n  --help: Display the help message for this command\n"),
        "{out}"
    );
    assert!(out.contains("\n  --version: Print the version\n"), "{out}");
    assert!(
        out.contains("-h, --host <string>: The host to use."),
        "{out}"
    );
    assert!(out.contains("-V, --verbose: Chatty output."), "{out}");
    assert!(!out.contains("-h, --help"), "{out}");
    assert!(!out.contains("-V, --version"), "{out}");
}

#[test]
fn the_user_short_flags_take_their_own_meaning() {
    let (code, out, err) = run(&["app", "-h", "box", "-V"]);
    assert!(code == ExitCode::SUCCESS, "{err}");
    assert_eq!(out, "");
    let (code, _, err) = run(&["app", "-h"]);
    assert!(code == ExitCode::from(2));
    assert!(err.contains("flag '-h' is missing its value"), "{err}");
}
