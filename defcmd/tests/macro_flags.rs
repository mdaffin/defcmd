//! End-to-end tests of value flags, short names and defaults through `try_run_with`.
//!
//! One root per process (the registry is global), so every test here uses this one tree.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Mutex;

static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn note(msg: impl Into<String>) {
    SEEN.lock().unwrap().push(msg.into());
}

fn seen(msg: &str) -> bool {
    SEEN.lock().unwrap().iter().any(|m| m == msg)
}

const JOBS: u32 = 4;

mod app {
    use super::{JOBS, note};
    use defcmd::command;
    use std::path::PathBuf;

    /// Build things.
    #[command]
    pub fn main() {}

    /// Compile a target.
    #[command]
    pub fn build(
        /// What to build.
        target: String,
        /// Parallel jobs.
        #[flag(-j)]
        #[default(JOBS)]
        jobs: u32,
        /// Where the output goes.
        #[flag(--out-dir, -o)]
        #[default("target")]
        dir: PathBuf,
        /// Print less.
        #[flag(-q)]
        quiet: bool,
        #[flag(--dry-run)] dry: bool,
        #[flag]
        #[default("none".to_string())]
        r#type: String,
        #[flag]
        #[default(-3)]
        offset: i64,
    ) {
        note(format!(
            "build {target} jobs={jobs} dir={} quiet={quiet} dry={dry} type={} offset={offset}",
            dir.display(),
            r#type
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
fn absent_flags_use_their_defaults() {
    ok(
        &["app", "build", "a"],
        "build a jobs=4 dir=target quiet=false dry=false type=none offset=-3",
    );
}

#[test]
fn flags_take_values_in_every_form() {
    ok(
        &[
            "app",
            "build",
            "a",
            "-j",
            "8",
            "-o",
            "out",
            "-q",
            "--dry-run",
        ],
        "build a jobs=8 dir=out quiet=true dry=true type=none offset=-3",
    );
    ok(
        &[
            "app",
            "build",
            "--jobs=0x10",
            "--out-dir",
            "o/p",
            "b",
            "--type=x",
        ],
        "build b jobs=16 dir=o/p quiet=false dry=false type=x offset=-3",
    );
    ok(
        &[
            "app", "build", "c", "--offset", "-5", "--jobs", "1", "--jobs", "2",
        ],
        "build c jobs=2 dir=target quiet=false dry=false type=none offset=-5",
    );
}

#[test]
fn explicit_long_name_replaces_the_derived_one() {
    usage_error(&["app", "build", "a", "--dir", "x"], "unknown flag '--dir'");
}

#[test]
fn a_raw_identifier_flag_loses_its_prefix() {
    ok(
        &["app", "build", "r", "--type", "t"],
        "build r jobs=4 dir=target quiet=false dry=false type=t offset=-3",
    );
}

#[test]
fn bad_and_missing_values_are_usage_errors() {
    usage_error(
        &["app", "build", "a", "--jobs", "x"],
        "invalid value 'x' for '--jobs': expected int",
    );
    usage_error(
        &["app", "build", "a", "-j", "5.0"],
        "invalid value '5.0' for '--jobs': expected int",
    );
    usage_error(
        &["app", "build", "a", "--jobs", "-1"],
        "invalid value '-1' for '--jobs': expected int",
    );
    usage_error(
        &["app", "build", "a", "-j"],
        "flag '-j' is missing its value",
    );
    usage_error(&["app", "build", "a", "-j8"], "unknown flag '-j8'");
    usage_error(&["app", "build", "a", "-j=8"], "unknown flag '-j=8'");
}

#[test]
fn the_function_stays_an_ordinary_function() {
    app::build(
        "z".into(),
        1,
        PathBuf::from("q"),
        false,
        false,
        "t".into(),
        0,
    );
    assert!(seen(
        "build z jobs=1 dir=q quiet=false dry=false type=t offset=0"
    ));
}

#[test]
fn generated_help_lists_flags_with_short_names_types_and_defaults() {
    let (code, out, err) = run(&["app", "build", "--help"]);
    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!(err, "");
    let expected = "\
Compile a target.

Usage:
  > app build {flags} <target>

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version
  -j, --jobs <int>: Parallel jobs. (default: 4)
  -o, --out-dir <path>: Where the output goes. (default: 'target')
  -q, --quiet: Print less.
  --dry-run
  --type <string> (default: 'none')
  --offset <int> (default: -3)

Parameters:
  target <string>: What to build.
";
    assert_eq!(out, expected);
}
