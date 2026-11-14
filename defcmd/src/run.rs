//! Running a program: find the root command, parse, and print help, version and errors.

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use crate::help::{render_help, render_usage_error, render_version};
use crate::parse::{Outcome, parse};
use crate::spec::{CommandSpec, Registration};
use crate::tree::Node;

/// Writes `text` and flushes. A closed pipe (`| head`) is not an error for defcmd's own output.
fn emit(w: &mut dyn Write, text: &str) {
    let _ = w.write_all(text.as_bytes());
    let _ = w.flush();
}

/// The program name: the file name of `argv[0]`.
fn program_name(argv0: Option<&OsString>) -> String {
    argv0
        .and_then(|a| Path::new(a).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "main".to_string())
}

fn usage_error(err: &mut dyn Write, spec: &CommandSpec, program: &str, message: &str) -> ExitCode {
    emit(err, &render_usage_error(spec, program, message));
    ExitCode::from(2)
}

/// Runs the command that `args` select among `registrations`: the root, or the subcommand named
/// by the leading words. The first item of `args` is the program name.
pub(crate) fn run_registrations(
    registrations: &[&Registration],
    args: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    let program = program_name(args.first());
    let Some(tree) = Node::build(registrations) else {
        emit(err, "Error: no root command is registered\n");
        return ExitCode::from(1);
    };

    let mut node = &tree;
    let mut rest: Vec<OsString> = args.get(1..).unwrap_or_default().to_vec();
    loop {
        let spec = node.reg.spec;
        match parse(spec, &rest, &|word| node.child_index(word)) {
            Ok(Outcome::Help) => {
                emit(out, &render_help(spec, &program));
                return ExitCode::SUCCESS;
            }
            Ok(Outcome::Version) => {
                emit(
                    out,
                    &render_version(&program, spec.version.unwrap_or_default()),
                );
                return ExitCode::SUCCESS;
            }
            Ok(Outcome::Run(parsed)) => {
                return match (node.reg.run)(&parsed, err) {
                    Ok(code) => code,
                    Err(e) => usage_error(err, spec, &program, &e.to_string()),
                };
            }
            Ok(Outcome::Sub { index, rest: after }) => {
                node = &node.children[index];
                rest = after;
            }
            Err(e) => return usage_error(err, spec, &program, &e.to_string()),
        }
    }
}

/// `args` with the program name replaced by `bin_name` when there is one (the `CARGO_BIN_NAME`
/// of the crate that has the root command), so help shows the Cargo target's name.
pub(crate) fn apply_bin_name(mut args: Vec<OsString>, bin_name: Option<&str>) -> Vec<OsString> {
    if let Some(name) = bin_name {
        match args.first_mut() {
            Some(argv0) => *argv0 = name.into(),
            None => args.push(name.into()),
        }
    }
    args
}

/// What the generated `fn main` calls.
pub(crate) fn main_with_bin_name(bin_name: Option<&'static str>) -> ExitCode {
    try_run(apply_bin_name(std::env::args_os().collect(), bin_name))
}

/// Like [`try_run`], but writes defcmd's own output (help, version, usage errors and the
/// `Error:` line for a returned `Err`) to `out` and `err`.
pub fn try_run_with<I, S>(args: I, out: &mut dyn Write, err: &mut dyn Write) -> ExitCode
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let registrations: Vec<&Registration> = inventory::iter::<Registration>.into_iter().collect();
    run_registrations(&registrations, &args, out, err)
}

/// Parses `args` (the first item is the program name) and runs the matching command, returning
/// the exit code instead of exiting. Uses the real stdout and stderr.
pub fn try_run<I, S>(args: I) -> ExitCode
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    try_run_with(args, &mut std::io::stdout(), &mut std::io::stderr())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{Parsed, UsageError};
    use crate::spec::{CommandSpec, ParamSpec};
    use crate::test_util::{documented, positional, switch};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static PARAMS: [ParamSpec; 2] = [
        documented(positional("name"), "Who to greet."),
        switch("shout", "shout"),
    ];
    static HELLO: CommandSpec = CommandSpec {
        path: &[],
        aliases: &[],
        doc: "Print a greeting.",
        params: &PARAMS,
        version: Some("1.0.0"),
    };

    static CALLS: AtomicUsize = AtomicUsize::new(0);

    fn glue_ok(parsed: &Parsed, _err: &mut dyn Write) -> Result<ExitCode, UsageError> {
        assert_eq!(parsed.get::<String>(0).unwrap(), "World");
        assert!(parsed.get_opt::<bool>(1).unwrap().is_none());
        CALLS.fetch_add(1, Ordering::SeqCst);
        Ok(ExitCode::SUCCESS)
    }

    fn glue_usage_error(_: &Parsed, _: &mut dyn Write) -> Result<ExitCode, UsageError> {
        Err(UsageError::new(
            "invalid value 'x' for 'name': expected int",
        ))
    }

    fn glue_unreachable(_: &Parsed, _: &mut dyn Write) -> Result<ExitCode, UsageError> {
        panic!("the command must not run");
    }

    static OK: Registration = Registration {
        spec: &HELLO,
        run: glue_ok,
    };
    static BAD_VALUE: Registration = Registration {
        spec: &HELLO,
        run: glue_usage_error,
    };
    static NEVER: Registration = Registration {
        spec: &HELLO,
        run: glue_unreachable,
    };

    fn run(reg: &Registration, args: &[&str]) -> (ExitCode, String, String) {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_registrations(&[reg], &args, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn runs_the_glue_with_the_parsed_arguments() {
        let before = CALLS.load(Ordering::SeqCst);
        let (code, out, err) = run(&OK, &["hello", "World"]);
        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!((out.as_str(), err.as_str()), ("", ""));
        assert_eq!(CALLS.load(Ordering::SeqCst), before + 1);
    }

    #[test]
    fn help_goes_to_stdout_with_exit_0() {
        let (code, out, err) = run(&NEVER, &["hello", "--help"]);
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.starts_with("Print a greeting.\n\nUsage:\n  > hello {flags} <name>\n"));
        assert_eq!(err, "");
    }

    #[test]
    fn version_goes_to_stdout_with_exit_0() {
        let (code, out, err) = run(&NEVER, &["hello", "-V"]);
        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!((out.as_str(), err.as_str()), ("hello 1.0.0\n", ""));
    }

    #[test]
    fn parse_errors_go_to_stderr_with_exit_2() {
        let (code, out, err) = run(&NEVER, &["hello"]);
        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert_eq!(
            err,
            "Error: missing required argument 'name'\n  \
             help: Usage: hello {flags} <name>. Use `hello --help` for more information.\n"
        );
    }

    #[test]
    fn value_errors_from_the_glue_are_usage_errors_too() {
        let (code, out, err) = run(&BAD_VALUE, &["hello", "x"]);
        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.starts_with("Error: invalid value 'x' for 'name': expected int\n  help: "));
    }

    #[test]
    fn program_name_is_the_file_name_of_argv0() {
        let (_, out, _) = run(&NEVER, &["/usr/local/bin/hello", "--version"]);
        assert_eq!(out, "hello 1.0.0\n");
    }

    #[test]
    fn a_missing_root_is_an_error() {
        let args = [OsString::from("hello")];
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_registrations(&[], &args, &mut out, &mut err);
        assert_eq!(code, ExitCode::from(1));
        assert!(out.is_empty());
        assert_eq!(
            String::from_utf8(err).unwrap(),
            "Error: no root command is registered\n"
        );
    }

    struct Closed;
    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
    }

    #[test]
    fn closed_output_does_not_panic_or_change_the_exit_code() {
        let run_closed = |args: &[&str]| {
            let args: Vec<OsString> = args.iter().map(OsString::from).collect();
            run_registrations(&[&NEVER], &args, &mut Closed, &mut Closed)
        };
        assert_eq!(run_closed(&["hello", "--help"]), ExitCode::SUCCESS);
        assert_eq!(run_closed(&["hello", "--version"]), ExitCode::SUCCESS);
        assert_eq!(run_closed(&["hello"]), ExitCode::from(2));
    }

    // The only test that goes through the global registry.
    static GLOBAL_SPEC: CommandSpec = CommandSpec {
        path: &[],
        aliases: &[],
        doc: "Global.",
        params: &[],
        version: Some("9.9.9"),
    };
    fn glue_global(_: &Parsed, _: &mut dyn Write) -> Result<ExitCode, UsageError> {
        Ok(ExitCode::from(7))
    }
    inventory::submit! {
        Registration { spec: &GLOBAL_SPEC, run: glue_global }
    }

    #[test]
    fn try_run_with_uses_the_registry() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = try_run_with(["prog"], &mut out, &mut err);
        assert_eq!(code, ExitCode::from(7));
        let code = try_run_with(["prog", "--version"], &mut out, &mut err);
        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(String::from_utf8(out).unwrap(), "prog 9.9.9\n");
    }

    #[test]
    fn bin_name_replaces_argv0() {
        let args = |a: &[&str]| a.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(
            apply_bin_name(args(&["target/debug/x", "a"]), Some("tool")),
            args(&["tool", "a"])
        );
        assert_eq!(
            apply_bin_name(args(&["target/debug/x", "a"]), None),
            args(&["target/debug/x", "a"])
        );
        assert_eq!(apply_bin_name(args(&[]), Some("tool")), args(&["tool"]));
    }
}

/// Dispatch through a hand-built tree shaped like the `errors.rs` example.
#[cfg(test)]
mod dispatch_tests {
    use super::*;
    use crate::parse::{Parsed, UsageError};
    use crate::spec::ParamSpec;
    use crate::test_util::{positional, switch};
    use std::sync::Mutex;

    /// Which glue ran, with the first positional it saw.
    static RAN: Mutex<Vec<String>> = Mutex::new(Vec::new());

    fn record(label: &str, parsed: &Parsed) -> Result<ExitCode, UsageError> {
        let first = if parsed.spec.params.is_empty() {
            String::new()
        } else {
            parsed
                .get_opt::<String>(0)
                .ok()
                .flatten()
                .unwrap_or_default()
        };
        RAN.lock().unwrap().push(format!("{label}:{first}"));
        Ok(ExitCode::SUCCESS)
    }
    fn ran(label: &str) -> bool {
        RAN.lock().unwrap().iter().any(|r| r == label)
    }

    fn glue_root(p: &Parsed, _: &mut dyn Write) -> Result<ExitCode, UsageError> {
        record("root", p)
    }
    fn glue_get(p: &Parsed, _: &mut dyn Write) -> Result<ExitCode, UsageError> {
        record("get", p)
    }
    fn glue_has(p: &Parsed, _: &mut dyn Write) -> Result<ExitCode, UsageError> {
        record("has", p)
    }

    static FILE_KEY: [ParamSpec; 2] = [positional("file"), positional("key")];
    static ROOT: CommandSpec = CommandSpec {
        path: &[],
        aliases: &[],
        doc: "Inspect config files.",
        params: &[],
        version: Some("1.0.0"),
    };
    static GET: CommandSpec = CommandSpec {
        path: &["get"],
        aliases: &[],
        doc: "Print a key.",
        params: &FILE_KEY,
        version: None,
    };
    static HAS: CommandSpec = CommandSpec {
        path: &["has"],
        aliases: &[],
        doc: "",
        params: &FILE_KEY,
        version: None,
    };
    static R_ROOT: Registration = Registration {
        spec: &ROOT,
        run: glue_root,
    };
    static R_GET: Registration = Registration {
        spec: &GET,
        run: glue_get,
    };
    static R_HAS: Registration = Registration {
        spec: &HAS,
        run: glue_has,
    };

    // A root that takes a positional itself, next to one child.
    static NAME_PARAMS: [ParamSpec; 2] = [positional("name"), switch("loud", "loud")];
    static GREET: CommandSpec = CommandSpec {
        path: &[],
        aliases: &[],
        doc: "",
        params: &NAME_PARAMS,
        version: None,
    };
    static LIST: CommandSpec = CommandSpec {
        path: &["list"],
        aliases: &[],
        doc: "",
        params: &[],
        version: None,
    };
    static R_GREET: Registration = Registration {
        spec: &GREET,
        run: glue_root,
    };
    static R_LIST: Registration = Registration {
        spec: &LIST,
        run: glue_get,
    };

    fn run(regs: &[&Registration], args: &[&str]) -> (ExitCode, String, String) {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_registrations(regs, &args, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    fn conf(args: &[&str]) -> (ExitCode, String, String) {
        run(&[&R_HAS, &R_ROOT, &R_GET], args)
    }

    #[test]
    fn a_subcommand_word_runs_that_command_with_the_rest() {
        let (code, out, err) = conf(&["conf", "get", "app.conf", "port"]);
        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!((out.as_str(), err.as_str()), ("", ""));
        assert!(ran("get:app.conf"));
        let (code, ..) = conf(&["conf", "has", "other.conf", "port"]);
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(ran("has:other.conf"));
    }

    #[test]
    fn no_words_run_the_root() {
        let (code, ..) = conf(&["conf"]);
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(ran("root:"));
    }

    #[test]
    fn an_unknown_word_without_a_positional_is_unexpected() {
        let (code, out, err) = conf(&["conf", "nosuch"]);
        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert_eq!(
            err,
            "Error: unexpected argument 'nosuch'\n  \
             help: Usage: conf. Use `conf --help` for more information.\n"
        );
    }

    #[test]
    fn a_word_that_is_no_child_goes_to_the_roots_positional() {
        let regs: &[&Registration] = &[&R_GREET, &R_LIST];
        let (code, ..) = run(regs, &["hi", "Ada"]);
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(ran("root:Ada"));
        // A flag before the word does not stop it naming a child.
        let (code, ..) = run(regs, &["hi", "--loud", "list"]);
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(ran("get:"));
    }

    #[test]
    fn usage_errors_name_the_subcommand() {
        let (code, out, err) = conf(&["conf", "get", "app.conf"]);
        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert_eq!(
            err,
            "Error: missing required argument 'key'\n  \
             help: Usage: conf get <file> <key>. Use `conf get --help` for more information.\n"
        );
        let (_, _, err) = conf(&["conf", "get", "a", "b", "extra"]);
        assert_eq!(
            err,
            "Error: unexpected argument 'extra'\n  \
             help: Usage: conf get <file> <key>. Use `conf get --help` for more information.\n"
        );
    }

    #[test]
    fn help_is_for_the_level_it_is_given_at() {
        let (code, out, err) = conf(&["conf", "--help"]);
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(
            out.starts_with("Inspect config files.\n\nUsage:\n  > conf\n"),
            "{out}"
        );
        assert_eq!(err, "");
        let (code, out, _) = conf(&["conf", "get", "--help"]);
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(
            out.starts_with("Print a key.\n\nUsage:\n  > conf get <file> <key>\n"),
            "{out}"
        );
        // `-h` after other arguments still wins over the missing ones.
        let (code, out, _) = conf(&["conf", "get", "app.conf", "-h"]);
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("> conf get <file> <key>"));
    }

    #[test]
    fn version_belongs_to_the_command_that_has_one() {
        let (code, out, _) = conf(&["conf", "--version"]);
        assert_eq!((code, out.as_str()), (ExitCode::SUCCESS, "conf 1.0.0\n"));
        let (code, _, err) = conf(&["conf", "get", "--version"]);
        assert_eq!(code, ExitCode::from(2));
        assert!(err.starts_with("Error: unknown flag '--version'"), "{err}");
    }
}
