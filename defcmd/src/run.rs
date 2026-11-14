//! Running a program: find the root command, parse, and print help, version and errors.

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use crate::help::{render_help, render_usage_error, render_version};
use crate::parse::{Outcome, parse};
use crate::spec::Registration;

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

/// Runs the root among `registrations` with `args` (the first item is the program name).
pub(crate) fn run_registrations(
    registrations: &[&Registration],
    args: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    let program = program_name(args.first());
    let Some(root) = registrations.iter().find(|r| r.spec.path.is_empty()) else {
        emit(err, "Error: no root command is registered\n");
        return ExitCode::from(1);
    };
    let spec = root.spec;
    let usage_error = |err: &mut dyn Write, message: &str| {
        emit(err, &render_usage_error(spec, &program, message));
        ExitCode::from(2)
    };

    // Subcommands (M5) are not looked up yet: no word names a child.
    match parse(spec, args.get(1..).unwrap_or_default(), &|_| None) {
        Ok(Outcome::Help) => {
            emit(out, &render_help(spec, &program));
            ExitCode::SUCCESS
        }
        Ok(Outcome::Version) => {
            emit(
                out,
                &render_version(&program, spec.version.unwrap_or_default()),
            );
            ExitCode::SUCCESS
        }
        Ok(Outcome::Run(parsed)) => match (root.run)(&parsed, err) {
            Ok(code) => code,
            Err(e) => usage_error(err, &e.to_string()),
        },
        Ok(Outcome::Sub { .. }) => usage_error(err, "unexpected argument"),
        Err(e) => usage_error(err, &e.to_string()),
    }
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
}
