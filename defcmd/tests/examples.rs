//! Runs the compiled example binaries and checks what they print.
//!
//! `cargo test` builds every example before running integration tests, so the binaries exist.
//! An example whose own `#[cfg(test)]` module should run needs
//! `[[example]] name = "..." test = true` in `Cargo.toml` (see `errors` once it is added).

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

/// What an example run produced.
#[derive(Debug)]
struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

/// Path of a built example: `target/<profile>/examples/<name>`. The test executable is in
/// `target/<profile>/deps` or, with cargo's newer build-dir layout, deep inside
/// `target/<profile>/build/`, so look in every ancestor directory.
fn example_path(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("test executable path");
    exe.ancestors()
        .map(|dir| dir.join("examples").join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("example `{name}` is not built (looked above {exe:?})"))
}

/// Runs example `example` with `argv[0]` set to `argv0`, so output uses the name the example's
/// docs use (`hello`, `copy`, ...) rather than the file name.
fn run(example: &str, argv0: &str, args: &[&str]) -> Run {
    let out = Command::new(example_path(example))
        .arg0(argv0)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("cannot run example `{example}`: {e}"));
    Run {
        code: out.status.code().expect("example was killed by a signal"),
        stdout: String::from_utf8(out.stdout).expect("stdout is UTF-8"),
        stderr: String::from_utf8(out.stderr).expect("stderr is UTF-8"),
    }
}

mod hello {
    use super::run;

    fn hello(args: &[&str]) -> super::Run {
        run("hello", "hello", args)
    }

    #[test]
    fn greets() {
        let r = hello(&["World"]);
        assert_eq!(
            (r.code, r.stdout.as_str(), r.stderr.as_str()),
            (0, "Hello, World!\n", "")
        );
    }

    #[test]
    fn shouts_with_the_switch_before_or_after() {
        for args in [["World", "--shout"], ["--shout", "World"]] {
            let r = hello(&args);
            assert_eq!(
                (r.code, r.stdout.as_str()),
                (0, "HELLO, WORLD!\n"),
                "{args:?}"
            );
        }
    }

    #[test]
    fn help() {
        let r = hello(&["--help"]);
        assert_eq!(r.code, 0);
        assert_eq!(r.stderr, "");
        assert_eq!(
            r.stdout,
            "\
Print a greeting.

Usage:
  > hello {flags} <name>

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version
  --shout: Print in upper case.

Parameters:
  name <string>: Who to greet.
"
        );
    }

    #[test]
    fn version() {
        let r = hello(&["--version"]);
        assert_eq!((r.code, r.stdout.as_str()), (0, "hello 0.1.0\n"));
    }

    #[test]
    fn missing_name_is_a_usage_error() {
        let r = hello(&[]);
        assert_eq!(r.code, 2);
        assert_eq!(r.stdout, "");
        assert_eq!(
            r.stderr,
            "Error: missing required argument 'name'\n  \
             help: Usage: hello {flags} <name>. Use `hello --help` for more information.\n"
        );
    }

    #[test]
    fn unknown_flag_is_a_usage_error() {
        let r = hello(&["World", "--nope"]);
        assert_eq!(r.code, 2);
        assert!(
            r.stderr.starts_with("Error: unknown flag '--nope'\n"),
            "{}",
            r.stderr
        );
    }

    #[test]
    fn extra_argument_is_a_usage_error() {
        let r = hello(&["World", "again"]);
        assert_eq!(r.code, 2);
        assert!(
            r.stderr.starts_with("Error: unexpected argument 'again'\n"),
            "{}",
            r.stderr
        );
    }
}

mod errors {
    use super::run;
    use std::path::PathBuf;

    fn conf(args: &[&str]) -> super::Run {
        run("conf", "conf", args)
    }

    /// A config file under the temp dir, named for the test so parallel tests do not clash.
    struct ConfigFile(PathBuf);

    impl ConfigFile {
        fn new(test: &str, text: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("defcmd-errors-{}-{test}.conf", std::process::id()));
            std::fs::write(&path, text).unwrap();
            ConfigFile(path)
        }

        fn arg(&self) -> &str {
            self.0.to_str().unwrap()
        }
    }

    impl Drop for ConfigFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    const TEXT: &str = "# comment\nport = 8080\n\nhost = example.test\n";

    #[test]
    fn root_without_arguments_prints_the_hint() {
        let r = conf(&[]);
        assert_eq!(
            (r.code, r.stdout.as_str(), r.stderr.as_str()),
            (0, "conf: try `conf --help`\n", "")
        );
    }

    #[test]
    fn check_counts_entries() {
        let file = ConfigFile::new("check", TEXT);
        let r = conf(&["check", file.arg()]);
        assert_eq!(r.code, 0);
        assert_eq!(r.stdout, format!("{}: 2 entries\n", file.arg()));
        assert_eq!(r.stderr, "");
    }

    #[test]
    fn check_of_a_missing_file_is_an_error_with_exit_1() {
        let r = conf(&["check", "/nonexistent/missing.conf"]);
        assert_eq!(r.code, 1);
        assert_eq!(r.stdout, "");
        assert!(
            r.stderr
                .starts_with("Error: cannot read /nonexistent/missing.conf: No such file"),
            "{}",
            r.stderr
        );
        assert!(!r.stderr.contains("help:"), "{}", r.stderr);
    }

    #[test]
    fn get_prints_the_value() {
        let file = ConfigFile::new("get-hit", TEXT);
        let r = conf(&["get", file.arg(), "port"]);
        assert_eq!(
            (r.code, r.stdout.as_str(), r.stderr.as_str()),
            (0, "8080\n", "")
        );
    }

    #[test]
    fn get_of_a_missing_key_is_an_error_with_exit_1() {
        let file = ConfigFile::new("get-miss", TEXT);
        let r = conf(&["get", file.arg(), "nope"]);
        assert_eq!(r.code, 1);
        assert_eq!(r.stdout, "");
        assert_eq!(
            r.stderr,
            format!("Error: key `nope` not found in {}\n", file.arg())
        );
    }

    #[test]
    fn has_is_silent_and_answers_with_the_exit_code() {
        let file = ConfigFile::new("has", TEXT);
        let yes = conf(&["has", file.arg(), "port"]);
        assert_eq!(
            (yes.code, yes.stdout.as_str(), yes.stderr.as_str()),
            (0, "", "")
        );
        let no = conf(&["has", file.arg(), "nope"]);
        assert_eq!(
            (no.code, no.stdout.as_str(), no.stderr.as_str()),
            (1, "", "")
        );
    }

    #[test]
    fn has_still_reports_an_unreadable_file() {
        let r = conf(&["has", "/nonexistent/missing.conf", "port"]);
        assert_eq!(r.code, 1);
        assert!(r.stderr.starts_with("Error: cannot read "), "{}", r.stderr);
    }

    #[test]
    fn get_without_a_key_is_a_usage_error() {
        let r = conf(&["get", "app.conf"]);
        assert_eq!(r.code, 2);
        assert_eq!(r.stdout, "");
        assert_eq!(
            r.stderr,
            "Error: missing required argument 'key'\n  \
             help: Usage: conf get <file> <key>. Use `conf get --help` for more information.\n"
        );
    }

    #[test]
    fn get_with_an_extra_argument_is_a_usage_error() {
        let r = conf(&["get", "app.conf", "port", "extra"]);
        assert_eq!(r.code, 2);
        assert_eq!(r.stdout, "");
        assert_eq!(
            r.stderr,
            "Error: unexpected argument 'extra'\n  \
             help: Usage: conf get <file> <key>. Use `conf get --help` for more information.\n"
        );
    }

    #[test]
    fn root_help_lists_the_subcommands() {
        let r = conf(&["--help"]);
        assert_eq!(r.code, 0);
        assert_eq!(r.stderr, "");
        assert_eq!(
            r.stdout,
            "\
Inspect simple `key = value` config files.

Usage:
  > conf

Subcommands:
  conf check - Check that a config file can be read.
  conf get - Print the value of a key.
  conf has - Exit 0 if the key is present, 1 if not. Prints nothing (like `grep -q`).

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version
"
        );
    }

    #[test]
    fn subcommand_help_shows_path_parameters() {
        let r = conf(&["get", "--help"]);
        assert_eq!(r.code, 0);
        assert_eq!(
            r.stdout,
            "\
Print the value of a key.

Usage:
  > conf get <file> <key>

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version

Parameters:
  file <path>: Config file.
  key <string>: Key to look up.
"
        );
    }
}

mod todo {
    use super::run;

    fn todo(args: &[&str]) -> super::Run {
        run("todo", "todo", args)
    }

    fn ok(args: &[&str], stdout: &str) {
        let r = todo(args);
        assert_eq!(
            (r.code, r.stdout.as_str(), r.stderr.as_str()),
            (0, stdout, ""),
            "{args:?}"
        );
    }

    fn usage_error(args: &[&str], message: &str) {
        let r = todo(args);
        assert_eq!(r.code, 2, "{args:?}");
        assert_eq!(r.stdout, "", "{args:?}");
        assert!(
            r.stderr.starts_with(&format!("Error: {message}\n")),
            "{args:?}: {}",
            r.stderr
        );
    }

    const ROOT_HELP: &str = "\
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

    const ADD_HELP: &str = "\
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

    const REMOTE_HELP: &str = "\
Usage:
  > todo remote

Subcommands:
  todo remote add - Add a remote list.
  todo remote remove - Remove a remote list.

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version
";

    #[test]
    fn no_subcommand_runs_main() {
        ok(&[], "todo (verbose=false); try `todo --help`\n");
        ok(&["-v"], "todo (verbose=true); try `todo --help`\n");
    }

    #[test]
    fn add_takes_a_title_and_a_priority() {
        ok(&["add", "buy milk"], "add \"buy milk\" priority=1\n");
        ok(
            &["add", "buy milk", "-p", "2"],
            "add \"buy milk\" priority=2\n",
        );
        ok(&["add", "--priority", "3", "x"], "add \"x\" priority=3\n");
    }

    #[test]
    fn list_takes_all() {
        ok(&["list"], "list all=false\n");
        ok(&["list", "--all"], "list all=true\n");
    }

    #[test]
    fn nested_commands() {
        ok(
            &["remote", "add", "origin", "https://example.com/repo"],
            "remote add origin https://example.com/repo\n",
        );
        ok(&["remote", "remove", "origin"], "remote remove origin\n");
    }

    #[test]
    fn a_group_prints_its_help_and_exits_2() {
        let r = todo(&["remote"]);
        assert_eq!(
            (r.code, r.stdout.as_str(), r.stderr.as_str()),
            (2, REMOTE_HELP, "")
        );
    }

    #[test]
    fn the_roots_flags_come_before_the_subcommand() {
        ok(&["--verbose", "list"], "list all=false\n");
        usage_error(&["list", "--verbose"], "unknown flag '--verbose'");
    }

    #[test]
    fn an_unknown_word_is_unexpected() {
        usage_error(&["nosuch"], "unexpected argument 'nosuch'");
        usage_error(&["remote", "nosuch"], "unexpected argument 'nosuch'");
    }

    #[test]
    fn after_double_dash_the_word_is_a_positional() {
        usage_error(&["--", "list"], "unexpected argument 'list'");
    }

    #[test]
    fn golden_help() {
        ok(&["--help"], ROOT_HELP);
        ok(&["add", "--help"], ADD_HELP);
        ok(&["remote", "--help"], REMOTE_HELP);
    }

    #[test]
    fn help_follows_the_flags_it_comes_after() {
        ok(&["--help", "list"], ROOT_HELP);
        let r = todo(&["list", "--help"]);
        assert!(r.stdout.starts_with("List items.\n"), "{}", r.stdout);
    }
}
