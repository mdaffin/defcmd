//! Runs the compiled example binaries and checks what they print.
//!
//! `cargo test` builds every example before running integration tests, so the binaries exist.
//! An example whose own `#[cfg(test)]` module should run needs
//! `[[example]] name = "..." test = true` in `Cargo.toml` (see `errors` once it is added).

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

/// What an example run produced.
#[allow(dead_code)]
#[derive(Debug)]
struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

/// Path of a built example: `target/<profile>/examples/<name>`, beside `target/<profile>/deps`.
#[allow(dead_code)]
fn example_path(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("test executable path");
    exe.parent()
        .and_then(|deps| deps.parent())
        .expect("target profile directory")
        .join("examples")
        .join(name)
}

/// Runs example `example` with `argv[0]` set to `argv0`, so output uses the name the example's
/// docs use (`hello`, `copy`, ...) rather than the file name.
#[allow(dead_code)]
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
