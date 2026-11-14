//! End-to-end tests of `#[parse(from_str)]` and `#[parse(path::to::fn)]` through `try_run_with`.
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
    use std::ffi::OsStr;
    use std::num::NonZeroU32;

    pub fn header(arg: &OsStr) -> Result<(String, String), String> {
        let s = arg.to_str().ok_or("not valid UTF-8")?;
        let (name, value) = s.split_once(':').ok_or("expected `Name: value`")?;
        Ok((name.trim().to_string(), value.trim().to_string()))
    }

    mod nested {
        use std::ffi::OsStr;
        pub fn first_char(arg: &OsStr) -> Result<char, std::str::ParseBoolError> {
            "true".parse::<bool>()?;
            Ok(arg.to_string_lossy().chars().next().unwrap_or('?'))
        }
    }

    /// Fetch.
    #[command]
    pub fn main(
        /// Positional with `FromStr`.
        #[parse(from_str)]
        count: NonZeroU32,
        /// Number of workers.
        #[flag]
        #[parse(from_str)]
        #[default(NonZeroU32::MIN)]
        workers: NonZeroU32,
        /// Optional port.
        #[flag]
        #[parse(from_str)]
        port: Option<NonZeroU32>,
        /// Many numbers.
        #[flag(-n)]
        #[parse(from_str)]
        nums: Vec<NonZeroU32>,
        /// Extra header.
        #[flag(--header, -H)]
        #[parse(header)]
        headers: Vec<(String, String)>,
        /// One header.
        #[flag]
        #[parse(header)]
        single: Option<(String, String)>,
        /// Letter.
        #[flag]
        #[parse(nested::first_char)]
        #[default('z')]
        letter: char,
        /// Rest.
        #[parse(from_str)]
        rest: Vec<NonZeroU32>,
    ) {
        note(format!(
            "count={count} workers={workers} port={port:?} nums={nums:?} headers={headers:?} \
             single={single:?} letter={letter} rest={rest:?}"
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
fn from_str_converts_every_shape() {
    ok(
        &[
            "x",
            "3",
            "--workers",
            "4",
            "--port",
            "80",
            "-n",
            "1",
            "-n",
            "2",
            "5",
            "6",
        ],
        "count=3 workers=4 port=Some(80) nums=[1, 2] headers=[] single=None letter=z rest=[5, 6]",
    );
}

#[test]
fn from_str_defaults_apply_when_absent() {
    ok(
        &["x", "7"],
        "count=7 workers=1 port=None nums=[] headers=[] single=None letter=z rest=[]",
    );
}

#[test]
fn from_str_errors_name_the_value_and_the_flag() {
    usage_error(
        &["x", "0"],
        "invalid value '0' for 'count': number would be zero for non-zero type",
    );
    usage_error(
        &["x", "1", "--workers", "0"],
        "invalid value '0' for '--workers': number would be zero for non-zero type",
    );
    usage_error(
        &["x", "1", "--port", "abc"],
        "invalid value 'abc' for '--port': invalid digit found in string",
    );
    usage_error(
        &["x", "1", "-n", "1", "-n", "q"],
        "invalid value 'q' for '--nums': invalid digit found in string",
    );
    usage_error(
        &["x", "1", "2", "0"],
        "invalid value '0' for 'rest': number would be zero for non-zero type",
    );
}

#[test]
fn custom_function_parses_inner_values() {
    ok(
        &[
            "x", "1", "-H", "A: b", "--header", "C:d", "--single", "K: v",
        ],
        "count=1 workers=1 port=None nums=[] headers=[(\"A\", \"b\"), (\"C\", \"d\")] \
         single=Some((\"K\", \"v\")) letter=z rest=[]",
    );
    ok(
        &["x", "1", "--letter", "q"],
        "count=1 workers=1 port=None nums=[] headers=[] single=None letter=q rest=[]",
    );
}

#[test]
fn custom_function_errors_use_its_message() {
    usage_error(
        &["x", "1", "-H", "nocolon"],
        "invalid value 'nocolon' for '--header': expected `Name: value`",
    );
    usage_error(
        &["x", "1", "--single", "nocolon"],
        "invalid value 'nocolon' for '--single': expected `Name: value`",
    );
}

#[test]
fn help_shows_value_names_and_defaults() {
    let (code, out, _) = run(&["x", "--help"]);
    assert_eq!(code, ExitCode::SUCCESS);
    for line in [
        "--workers <nonzero>: Number of workers. (default: 1)",
        "--port <nonzero>: Optional port.",
        "-n, --nums <nonzero>: Many numbers. (repeatable)",
        "-H, --header <value>: Extra header. (repeatable)",
        "--letter <char>: Letter. (default: z)",
    ] {
        assert!(out.contains(line), "missing {line:?} in:\n{out}");
    }
}
