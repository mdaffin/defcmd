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
        // Read the expected version from the package rather than hardcoding it, so the test
        // doesn't break on every version bump.
        let version = env!("CARGO_PKG_VERSION");
        let r = hello(&["--version"]);
        assert_eq!(
            (r.code, r.stdout.as_str()),
            (0, format!("hello {version}\n").as_str())
        );
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
  -v, --verbose: Print extra detail.

Parameters:
  title <string>: What needs doing.
";

    const REMOTE_HELP: &str = "\
Usage:
  > todo remote {flags}

Subcommands:
  todo remote add - Add a remote list.
  todo remote remove - Remove a remote list.

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version
  -v, --verbose: Print extra detail.
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
        ok(&["list"], "list all=false verbose=false\n");
        ok(&["list", "--all"], "list all=true verbose=false\n");
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
    fn a_group_prints_its_help_to_stderr_and_exits_2() {
        let r = todo(&["remote"]);
        assert_eq!(
            (r.code, r.stdout.as_str(), r.stderr.as_str()),
            (2, "", REMOTE_HELP)
        );
    }

    #[test]
    fn a_global_flag_comes_before_or_after_the_subcommand_word() {
        ok(&["--verbose", "list"], "list all=false verbose=true\n");
        ok(&["list", "--verbose"], "list all=false verbose=true\n");
        ok(&["-v", "list", "-a"], "list all=true verbose=true\n");
        // `add` does not read it, but accepts it.
        ok(&["add", "x", "-v"], "add \"x\" priority=1\n");
        ok(&["--verbose", "add", "x"], "add \"x\" priority=1\n");
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

mod deploy {
    use super::run;

    fn deploy(args: &[&str]) -> super::Run {
        run("deploy", "deploy", args)
    }

    fn ok(args: &[&str], stdout: &str) {
        let r = deploy(args);
        assert_eq!(
            (r.code, r.stdout.as_str(), r.stderr.as_str()),
            (0, stdout, ""),
            "{args:?}"
        );
    }

    #[test]
    fn the_root_sees_the_global_flags() {
        ok(
            &[],
            "deploy verbose=false dry_run=false env=staging local=false\n",
        );
        ok(
            &["--dry-run", "-e", "prod", "-v", "--local"],
            "deploy verbose=true dry_run=true env=prod local=true\n",
        );
    }

    #[test]
    fn a_global_flag_comes_before_or_after_the_subcommand_word() {
        ok(&["push", "api"], "push api to staging\n");
        for args in [
            &["push", "api", "--env", "prod"][..],
            &["--env", "prod", "push", "api"],
            &["push", "--env=prod", "api"],
            &["-e", "prod", "push", "api"],
        ] {
            ok(args, "push api to prod\n");
        }
        ok(
            &["--dry-run", "push", "api", "-e", "prod"],
            "would push api to prod\n",
        );
    }

    #[test]
    fn a_command_that_does_not_repeat_a_global_flag_still_accepts_it() {
        ok(&["-v", "status"], "status\n");
        ok(&["status", "--env", "prod", "--dry-run"], "status\n");
    }

    #[test]
    fn a_root_flag_that_is_not_global_cannot_come_before_a_subcommand_word() {
        let r = deploy(&["--local", "push", "api"]);
        assert_eq!((r.code, r.stdout.as_str()), (2, ""));
        assert!(
            r.stderr
                .starts_with("Error: flag '--local' has no effect before the subcommand 'push'\n"),
            "{}",
            r.stderr
        );
        let r = deploy(&["push", "api", "--local"]);
        assert_eq!(r.code, 2);
        assert!(r.stderr.starts_with("Error: unknown flag '--local'\n"));
    }

    #[test]
    fn help_lists_the_globals_a_command_declares_and_those_it_only_accepts() {
        let expected = "\
Show what is deployed.

Usage:
  > deploy status {flags}

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version
  -v, --verbose: Print extra detail.
  --dry-run: Show what would happen without doing it.
  -e, --env <string>: Environment to target. (default: 'staging')
";
        let r = deploy(&["status", "--help"]);
        assert_eq!(
            (r.code, r.stdout.as_str(), r.stderr.as_str()),
            (0, expected, "")
        );
        let r = deploy(&["push", "--help"]);
        assert_eq!(r.stdout.matches("--env").count(), 1, "{}", r.stdout);
        assert_eq!(r.stdout.matches("--dry-run").count(), 1, "{}", r.stdout);
    }
}

mod copy {
    use super::run;

    fn copy(args: &[&str]) -> super::Run {
        run("copy", "copy", args)
    }

    const DEFAULTS: &str =
        "force=false dry_run=false jobs=4 retries=None color=true exclude=[] links=None";

    /// The line `copy` prints for `from` -> `to`, with `changes` applied to the default flags.
    fn copied(from: &str, to: &str, changes: &[(&str, &str)]) -> String {
        let mut flags = DEFAULTS.to_string();
        for (from, to) in changes {
            assert!(flags.contains(from), "{from} is not in {flags}");
            flags = flags.replace(from, to);
        }
        format!("copy {from:?} -> {to:?} {flags}\n")
    }

    fn ok(args: &[&str], expected: &str) {
        let r = copy(args);
        assert_eq!(
            (r.code, r.stdout.as_str(), r.stderr.as_str()),
            (0, expected, ""),
            "{args:?}"
        );
    }

    fn usage_error(args: &[&str], message: &str) {
        let r = copy(args);
        assert_eq!(r.code, 2, "{args:?}");
        assert_eq!(r.stdout, "", "{args:?}");
        assert!(
            r.stderr.starts_with(&format!("Error: {message}\n")),
            "{args:?}: {}",
            r.stderr
        );
    }

    #[test]
    fn the_command_list_of_the_docs() {
        ok(&["a.txt"], &copied("a.txt", ".", &[]));
        ok(
            &["a.txt", "b.txt", "--force", "-j", "8"],
            &copied(
                "a.txt",
                "b.txt",
                &[("force=false", "force=true"), ("jobs=4", "jobs=8")],
            ),
        );
        ok(
            &["a.txt", "b.txt", "--retries", "5", "--no-color"],
            &copied(
                "a.txt",
                "b.txt",
                &[
                    ("retries=None", "retries=Some(5)"),
                    ("color=true", "color=false"),
                ],
            ),
        );
        ok(
            &["--dry-run", "a.txt", "b.txt"],
            &copied("a.txt", "b.txt", &[("dry_run=false", "dry_run=true")]),
        );
        let removed = "\
remove \"a.txt\" recursive=true force=false
remove \"b.txt\" recursive=true force=false
remove \"c.txt\" recursive=true force=false
";
        ok(
            &["remove", "a.txt", "b.txt", "c.txt", "--recursive"],
            removed,
        );
        ok(&["rm", "a.txt", "b.txt", "c.txt", "--recursive"], removed);
    }

    #[test]
    fn flag_syntax_lines_of_the_docs() {
        let jobs8 = copied("a.txt", ".", &[("jobs=4", "jobs=8")]);
        ok(&["a.txt", "--jobs", "8"], &jobs8);
        ok(&["a.txt", "--jobs=8"], &jobs8);
        ok(&["a.txt", "-j", "8"], &jobs8);
        usage_error(
            &["a.txt", "-j8"],
            "flag '-j' takes a value, so it must be last in a bundle",
        );
        usage_error(&["a.txt", "-j=8"], "unknown flag '-j=8'");

        let rf = "remove \"a.txt\" recursive=true force=true\nremove \"b.txt\" recursive=true force=true\n";
        ok(&["remove", "-rf", "a.txt", "b.txt"], rf);
        ok(&["rm", "-rf", "a.txt", "b.txt"], rf);
        ok(
            &["a.txt", "-fj", "8"],
            &copied(
                "a.txt",
                ".",
                &[("force=false", "force=true"), ("jobs=4", "jobs=8")],
            ),
        );
        usage_error(
            &["a.txt", "-jf", "8"],
            "flag '-j' takes a value, so it must be last in a bundle",
        );
        ok(
            &["a.txt", "-j", "1", "-j", "2"],
            &copied("a.txt", ".", &[("jobs=4", "jobs=2")]),
        );
        ok(&["a.txt", "--force=false"], &copied("a.txt", ".", &[]));
        usage_error(
            &["a.txt", "--force=yes"],
            "invalid value 'yes' for '--force': expected bool",
        );
        let odd = "remove \"-odd-name.txt\" recursive=false force=false\n";
        ok(&["remove", "--", "-odd-name.txt"], odd);
        ok(&["rm", "--", "-odd-name.txt"], odd);
        // A file called `remove`: after `--` it is not the subcommand.
        ok(&["--", "remove"], &copied("remove", ".", &[]));
        usage_error(&["-5"], "unknown flag '-5'");
        ok(&["--", "-5"], &copied("-5", ".", &[]));
        usage_error(
            &["a.txt", "--jobs", "-1"],
            "invalid value '-1' for '--jobs': expected int",
        );
        usage_error(
            &["a.txt", "--jobs", "--force"],
            "invalid value '--force' for '--jobs': expected int",
        );
        usage_error(&["a.txt", "--jobs"], "flag '--jobs' is missing its value");
    }

    #[test]
    fn repeated_negatable_and_tri_state_lines_of_the_docs() {
        ok(
            &["a.txt", "-x", "*.tmp", "-x", "*.bak"],
            &copied(
                "a.txt",
                ".",
                &[("exclude=[]", "exclude=[\"*.tmp\", \"*.bak\"]")],
            ),
        );
        let no_color = copied("a.txt", ".", &[("color=true", "color=false")]);
        ok(&["a.txt", "--no-color"], &no_color);
        ok(&["a.txt", "--color=false"], &no_color);
        ok(&["a.txt", "--color"], &copied("a.txt", ".", &[]));

        let yes = copied("a.txt", ".", &[("links=None", "links=Some(true)")]);
        let no = copied("a.txt", ".", &[("links=None", "links=Some(false)")]);
        ok(&["a.txt", "--links"], &yes);
        ok(&["a.txt", "--no-links"], &no);
        ok(&["a.txt", "--links=false"], &no);
        ok(&["a.txt"], &copied("a.txt", ".", &[]));
        // `false` is not the value of `--links`: it is the `to` positional.
        ok(
            &["a.txt", "--links", "false"],
            &copied("a.txt", "false", &[("links=None", "links=Some(true)")]),
        );
    }

    #[test]
    fn an_empty_rest_does_nothing() {
        ok(&["remove"], "");
    }

    const COPY_HELP: &str = "\
Copy a file.

Usage:
  > copy {flags} <from> (to)

Subcommands:
  copy remove, rm - Remove files.

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version
  -f, --force: Overwrite the destination if it exists.
  --dry-run: Show what would happen without doing it.
  -j, --jobs <int>: Number of parallel jobs. (default: 4)
  --retries <int>: Retry count (unset means never retry).
  --color, --no-color: Coloured output; `--no-color` turns it off. (default: true)
  -x, --exclude <string>: Skip files matching a pattern. (repeatable)
  --links, --no-links: Follow symlinks (unset means use the default behaviour).

Parameters:
  from <path>: Source file.
  to <path>: Destination; the current directory if omitted. (optional, default: '.')
";

    const REMOVE_HELP: &str = "\
Remove files.

Usage:
  > copy remove {flags} ...(files)

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version
  -r, --recursive: Descend into directories.
  -f, --force: Ignore files that do not exist.

Parameters:
  ...files <path>: Files to remove.
";

    #[test]
    fn golden_help() {
        ok(&["--help"], COPY_HELP);
        ok(&["remove", "--help"], REMOVE_HELP);
        ok(&["rm", "--help"], REMOVE_HELP);
    }

    #[test]
    fn help_beats_other_usage_errors() {
        ok(&["--help", "--nope"], COPY_HELP);
        ok(&["--nope", "-h"], COPY_HELP);
    }
}

mod fetch {
    use super::run;

    fn fetch(args: &[&str]) -> super::Run {
        run("fetch", "fetch", args)
    }

    /// Runs `fetch` and checks the exit code and stdout of a success.
    fn ok(args: &[&str], stdout: &str) {
        let r = fetch(args);
        assert_eq!(
            (r.code, r.stdout.as_str(), r.stderr.as_str()),
            (0, stdout, ""),
            "{args:?}"
        );
    }

    /// Runs `fetch` and checks that it is a usage error (exit 2) with this message.
    fn bad(args: &[&str], message: &str) {
        let r = fetch(args);
        assert_eq!(r.code, 2, "{args:?}: {}", r.stderr);
        assert_eq!(r.stdout, "", "{args:?}");
        assert!(
            r.stderr.starts_with(&format!("Error: {message}\n")),
            "{args:?}: {}",
            r.stderr
        );
    }

    #[test]
    fn defaults() {
        ok(
            &["https://example.com"],
            "fetch https://example.com timeout=30s mode=Fast retries=3 workers=1 bind=None \
             cache=None headers=[]\n",
        );
    }

    #[test]
    fn durations_integers_and_enums() {
        ok(
            &[
                "https://example.com",
                "--timeout",
                "1.5min",
                "--retries",
                "0x10",
                "--mode",
                "careful",
            ],
            "fetch https://example.com timeout=90s mode=Careful retries=16 workers=1 bind=None \
             cache=None headers=[]\n",
        );
    }

    #[test]
    fn addresses_and_custom_types() {
        ok(
            &[
                "https://example.com",
                "--bind",
                "127.0.0.1:8080",
                "--cache",
                "50%",
            ],
            "fetch https://example.com timeout=30s mode=Fast retries=3 workers=1 \
             bind=Some(127.0.0.1:8080) cache=Some(Percent(50)) headers=[]\n",
        );
        ok(
            &["https://example.com", "--cache", "7"],
            "fetch https://example.com timeout=30s mode=Fast retries=3 workers=1 bind=None \
             cache=Some(Percent(7)) headers=[]\n",
        );
    }

    #[test]
    fn repeated_headers_use_the_custom_parser() {
        ok(
            &[
                "https://example.com",
                "-H",
                "Accept: */*",
                "--header",
                "X-Id: 7",
            ],
            "fetch https://example.com timeout=30s mode=Fast retries=3 workers=1 bind=None \
             cache=None headers=[(\"Accept\", \"*/*\"), (\"X-Id\", \"7\")]\n",
        );
    }

    #[test]
    fn a_duration_needs_a_unit() {
        bad(
            &["https://example.com", "--timeout", "30"],
            "invalid value '30' for '--timeout': expected duration, like 30sec or 250ms",
        );
    }

    #[test]
    fn a_bad_mode_lists_the_choices() {
        bad(
            &["https://example.com", "--mode", "slow"],
            "invalid value 'slow' for '--mode': expected one of: fast, careful",
        );
    }

    #[test]
    fn zero_workers_is_rejected_by_non_zero() {
        bad(
            &["https://example.com", "--workers", "0"],
            "invalid value '0' for '--workers': number would be zero for non-zero type",
        );
    }

    #[test]
    fn a_bad_header_reports_the_custom_message() {
        bad(
            &["https://example.com", "-H", "nocolon"],
            "invalid value 'nocolon' for '--header': expected `Name: value`",
        );
    }

    #[test]
    fn a_user_type_reports_its_own_error() {
        bad(
            &["https://example.com", "--cache", "101"],
            "invalid value '101' for '--cache': must be between 0 and 100",
        );
        bad(
            &["https://example.com", "--bind", "nope"],
            "invalid value 'nope' for '--bind': expected socket address, like 127.0.0.1:80",
        );
    }

    const HELP: &str = "\
Fetch a URL.

Usage:
  > fetch {flags} <url>

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version
  --timeout <duration>: Give up after this long. (default: 30sec)
  --mode <fast|careful>: How hard to try. (default: 'fast')
  --retries <int>: Retry count. (default: 3)
  --workers <nonzero>: Number of workers (at least 1). (default: 1)
  --bind <socketaddr>: Local address to bind.
  --cache <percent>: How much of the response to cache.
  -H, --header <value>: Extra request header (repeatable). (repeatable)

Parameters:
  url <string>: What to fetch.
";

    #[test]
    fn golden_help() {
        ok(&["--help"], HELP);
    }
}

/// Start-up validation of the command tree: the one place a deliberate panic is
/// tested end to end.
mod clashing {
    use super::run;

    #[test]
    fn an_invalid_tree_panics_with_every_problem_and_exit_101() {
        for args in [&[][..], &["get"], &["--help"]] {
            let r = run("clashing", "clashing", args);
            assert_eq!((r.code, r.stdout.as_str()), (101, ""), "{args:?}");
            assert!(r.stderr.contains("invalid command tree:\n"), "{}", r.stderr);
            assert!(
                r.stderr
                    .contains("  - command 'get' is registered 2 times: clashing::first ("),
                "{}",
                r.stderr
            );
            assert!(
                r.stderr
                    .contains("clashing::second (defcmd/tests/fixtures/clashing.rs:")
            );
            assert!(
                r.stderr.contains(
                    "  - the name 'remote' is used by both command 'push' (clashing::push ("
                ),
                "{}",
                r.stderr
            );
            assert!(
                r.stderr
                    .contains("group 'remote' (implied by clashing::remote_add ("),
                "{}",
                r.stderr
            );
        }
    }
}
