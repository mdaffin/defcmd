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
        let spec = node.spec;
        match parse(spec, &rest, &|word| node.child_index(word)) {
            Ok(Outcome::Help) => {
                emit(out, &render_help(spec, &program, &node.descendants()));
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
                let Some(reg) = node.reg else {
                    // An implicit group has no body: show its help, but as a failure.
                    emit(out, &render_help(spec, &program, &node.descendants()));
                    return ExitCode::from(2);
                };
                return match (reg.run)(&parsed, err) {
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
    use crate::test_util::{documented, flag, positional, switch, with_short};
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

    static REMOTE_ADD: CommandSpec = CommandSpec {
        path: &["remote", "add"],
        aliases: &[],
        doc: "Add a remote.",
        params: &[],
        version: None,
    };
    static R_REMOTE_ADD: Registration = Registration {
        spec: &REMOTE_ADD,
        run: glue_get,
    };

    fn grouped(args: &[&str]) -> (ExitCode, String, String) {
        run(&[&R_ROOT, &R_REMOTE_ADD], args)
    }

    #[test]
    fn a_group_alone_prints_its_help_to_stdout_and_fails_with_2() {
        let (code, out, err) = grouped(&["conf", "remote"]);
        assert_eq!(code, ExitCode::from(2));
        let expected = "\
Usage:
  > conf remote

Subcommands:
  conf remote add - Add a remote.

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version
";
        assert_eq!(out, expected);
        assert_eq!(err, "");
    }

    #[test]
    fn a_group_with_help_exits_0_and_runs_its_children() {
        let (code, out, _) = grouped(&["conf", "remote", "--help"]);
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.starts_with("Usage:\n  > conf remote\n"), "{out}");
        let (code, _, err) = grouped(&["conf", "remote", "add"]);
        assert_eq!((code, err.as_str()), (ExitCode::SUCCESS, ""));
        assert!(ran("get:"));
    }

    #[test]
    fn a_group_rejects_a_word_that_is_not_a_child() {
        let (code, out, err) = grouped(&["conf", "remote", "nosuch"]);
        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(
            err.starts_with("Error: unexpected argument 'nosuch'\n"),
            "{err}"
        );
        assert!(err.contains("Usage: conf remote."), "{err}");
    }

    #[test]
    fn the_root_lists_group_children_flattened_without_the_group() {
        let (_, out, _) = grouped(&["conf", "--help"]);
        assert!(
            out.contains("Subcommands:\n  conf remote add - Add a remote.\n"),
            "{out}"
        );
    }

    // A root with `--verbose` and a value flag `--prio`, one child with its own flags. Their
    // glue writes its own name to `err`, so a test sees which body ran without shared state.
    fn glue_todo(_: &Parsed, err: &mut dyn Write) -> Result<ExitCode, UsageError> {
        emit(err, "ran todo");
        Ok(ExitCode::SUCCESS)
    }
    fn glue_todo_list(_: &Parsed, err: &mut dyn Write) -> Result<ExitCode, UsageError> {
        emit(err, "ran list");
        Ok(ExitCode::SUCCESS)
    }

    static TODO_PARAMS: [ParamSpec; 2] = [switch("verbose", "verbose"), flag("prio", "prio")];
    static TODO: CommandSpec = CommandSpec {
        path: &[],
        aliases: &[],
        doc: "A tiny todo list.",
        params: &TODO_PARAMS,
        version: Some("1.0.0"),
    };
    static LIST_PARAMS: [ParamSpec; 1] = [switch("all", "all")];
    static TODO_LIST: CommandSpec = CommandSpec {
        path: &["list"],
        aliases: &[],
        doc: "List items.",
        params: &LIST_PARAMS,
        version: None,
    };
    static R_TODO: Registration = Registration {
        spec: &TODO,
        run: glue_todo,
    };
    static R_TODO_LIST: Registration = Registration {
        spec: &TODO_LIST,
        run: glue_todo_list,
    };

    fn todo(args: &[&str]) -> (ExitCode, String, String) {
        run(&[&R_TODO, &R_TODO_LIST], args)
    }

    #[test]
    fn the_roots_flags_may_come_before_the_subcommand_word() {
        let (code, _, err) = todo(&["todo", "--verbose", "list"]);
        // Only the child's body ran.
        assert_eq!((code, err.as_str()), (ExitCode::SUCCESS, "ran list"));
    }

    #[test]
    fn flags_are_not_inherited() {
        let (code, _, err) = todo(&["todo", "list", "--verbose"]);
        assert_eq!(code, ExitCode::from(2));
        assert!(
            err.starts_with("Error: unknown flag '--verbose'\n"),
            "{err}"
        );
        assert!(err.contains("Usage: todo list {flags}"), "{err}");
        assert!(!err.contains("ran"), "{err}");
    }

    #[test]
    fn a_value_flag_before_the_word_consumes_the_next_argument() {
        // `list` was the value of `--prio`, so no subcommand is chosen: the root runs.
        let (code, _, err) = todo(&["todo", "--prio", "list"]);
        assert_eq!((code, err.as_str()), (ExitCode::SUCCESS, "ran todo"));
        let (code, _, err) = todo(&["todo", "--prio", "3", "list"]);
        assert_eq!((code, err.as_str()), (ExitCode::SUCCESS, "ran list"));
    }

    #[test]
    fn after_double_dash_the_word_is_a_positional() {
        let (code, _, err) = todo(&["todo", "--", "list"]);
        assert_eq!(code, ExitCode::from(2));
        assert!(
            err.starts_with("Error: unexpected argument 'list'\n"),
            "{err}"
        );
        assert!(!err.contains("ran"), "{err}");
    }

    #[test]
    fn the_subcommand_word_must_match_exactly() {
        for word in ["lis", "List", "list2"] {
            let (code, _, err) = todo(&["todo", word]);
            assert_eq!(code, ExitCode::from(2), "{word}");
            assert!(
                err.contains(&format!("unexpected argument '{word}'")),
                "{err}"
            );
        }
    }

    #[test]
    fn help_belongs_to_the_command_whose_flags_it_follows() {
        let (code, out, err) = todo(&["todo", "--help", "list"]);
        assert_eq!((code, err.as_str()), (ExitCode::SUCCESS, ""));
        assert!(out.starts_with("A tiny todo list.\n"), "{out}");
        let (code, out, err) = todo(&["todo", "list", "--help"]);
        assert_eq!((code, err.as_str()), (ExitCode::SUCCESS, ""));
        assert!(out.starts_with("List items.\n"), "{out}");
    }

    #[test]
    fn the_root_body_runs_only_when_no_subcommand_is_chosen() {
        let (code, _, err) = todo(&["todo", "--verbose"]);
        assert_eq!((code, err.as_str()), (ExitCode::SUCCESS, "ran todo"));
    }

    // The tree of `examples/nushell_style.rs`, built by hand, for its golden help texts.
    fn priority_default() -> Option<String> {
        crate::value::FromArg::display_arg(&1u8)
    }
    static VERBOSE_PARAMS: [ParamSpec; 1] = [with_short(
        documented(switch("verbose", "verbose"), "Print extra detail."),
        'v',
    )];
    static ADD_PARAMS: [ParamSpec; 2] = [
        documented(positional("title"), "What needs doing."),
        ParamSpec {
            type_name: <u8 as crate::value::FromArg>::type_name,
            possible_values: <u8 as crate::value::FromArg>::possible_values,
            default_display: Some(priority_default),
            ..with_short(
                documented(flag("priority", "priority"), "1 (low) to 3 (high)."),
                'p',
            )
        },
    ];
    const fn command(
        path: &'static [&'static str],
        doc: &'static str,
        params: &'static [ParamSpec],
    ) -> CommandSpec {
        CommandSpec {
            path,
            aliases: &[],
            doc,
            params,
            version: Some("1.0.0"),
        }
    }
    static NS_MAIN: CommandSpec = command(&[], "A tiny todo list.", &VERBOSE_PARAMS);
    static NS_ADD: CommandSpec = command(&["add"], "Add an item.", &ADD_PARAMS);
    static NS_LIST: CommandSpec = command(&["list"], "List items.", &[]);
    static NS_REMOTE_ADD: CommandSpec = command(&["remote", "add"], "Add a remote list.", &[]);
    static NS_REMOTE_REMOVE: CommandSpec =
        command(&["remote", "remove"], "Remove a remote list.", &[]);
    static R_NS: [Registration; 5] = [
        Registration {
            spec: &NS_REMOTE_REMOVE,
            run: glue_todo_list,
        },
        Registration {
            spec: &NS_LIST,
            run: glue_todo_list,
        },
        Registration {
            spec: &NS_MAIN,
            run: glue_todo,
        },
        Registration {
            spec: &NS_REMOTE_ADD,
            run: glue_todo_list,
        },
        Registration {
            spec: &NS_ADD,
            run: glue_todo_list,
        },
    ];

    fn nushell(args: &[&str]) -> (ExitCode, String, String) {
        let regs: Vec<&Registration> = R_NS.iter().collect();
        run(&regs, args)
    }

    #[test]
    fn golden_root_help_lists_every_descendant_flattened_and_sorted() {
        let expected = "\
A tiny todo list.

Usage:
  > todo {flags}

Subcommands:
  todo add - Add an item.
  todo list - List items.
  todo remote add - Add a remote list.
  todo remote remove - Remove a remote list.

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version
  -v, --verbose: Print extra detail.
";
        let (code, out, err) = nushell(&["todo", "--help"]);
        assert_eq!((code, err.as_str()), (ExitCode::SUCCESS, ""));
        assert_eq!(out, expected);
    }

    #[test]
    fn golden_subcommand_help_has_the_full_path_in_usage() {
        let expected = "\
Add an item.

Usage:
  > todo add {flags} <title>

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version
  -p, --priority <int>: 1 (low) to 3 (high). (default: 1)

Parameters:
  title <string>: What needs doing.
";
        let (code, out, err) = nushell(&["todo", "add", "--help"]);
        assert_eq!((code, err.as_str()), (ExitCode::SUCCESS, ""));
        assert_eq!(out, expected);
    }

    #[test]
    fn golden_group_help_starts_at_usage_and_lists_its_own_descendants() {
        let expected = "\
Usage:
  > todo remote

Subcommands:
  todo remote add - Add a remote list.
  todo remote remove - Remove a remote list.

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version
";
        let (code, out, err) = nushell(&["todo", "remote", "--help"]);
        assert_eq!((code, err.as_str()), (ExitCode::SUCCESS, ""));
        assert_eq!(out, expected);
        // Without `--help` the same text is printed, with exit 2 and no error line.
        let (code, out, err) = nushell(&["todo", "remote"]);
        assert_eq!((code, err.as_str()), (ExitCode::from(2), ""));
        assert_eq!(out, expected);
    }
}
