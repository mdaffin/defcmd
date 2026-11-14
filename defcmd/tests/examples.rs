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
