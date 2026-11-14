//! A command is a plain function marked [`command`]; its signature is the command line.
//!
//! defcmd is modelled on nushell's `def main [...]`: parameters, flags, subcommands, help text
//! and error output follow nushell's conventions. There is no builder API and no boilerplate.
//!
//! ```no_run
//! use defcmd::command;
//!
//! /// Print a greeting.
//! #[command]
//! fn main(
//!     /// Who to greet.
//!     name: String,
//!     /// Print in upper case.
//!     #[flag]
//!     shout: bool,
//! ) {
//!     let msg = format!("Hello, {name}!");
//!     println!("{}", if shout { msg.to_uppercase() } else { msg });
//! }
//! ```
//!
//! `fn main` is the root command and the program entry point. Any other `#[command]` function
//! is a subcommand, named after the function or by a path such as `#[command("remote add")]`.
//! Doc comments on the command and on its parameters are the help text.
//!
//! # Parameters
//!
//! | Declaration | Meaning |
//! |---|---|
//! | `x: T` | required positional |
//! | `x: Option<T>` | optional positional |
//! | `#[default(expr)] x: T` | optional positional with a default |
//! | `x: Vec<T>` or `#[rest] x: Vec<T>` | variadic positionals, last |
//! | `#[flag] x: bool` | switch |
//! | `#[flag] x: Option<T>` | value flag |
//! | `#[flag] #[default(expr)] x: T` | value flag with a default |
//! | `#[flag] x: Vec<T>` | repeatable value flag |
//! | `#[flag] x: Option<bool>` | tri-state switch (also `--no-x`) |
//! | `#[flag(global)] x: T` | global flag, on the root and repeated on the commands that read it |
//!
//! `#[flag(-j)]`, `#[flag(--jobs)]` and `#[flag(--jobs, -j)]` choose the short and long names.
//! Values are parsed with [`FromArg`], which is provided for strings, paths, integers, floats,
//! `bool`, `char`, `Duration` and the `std::net` addresses. `#[derive(FromArg)]` handles
//! fieldless enums, and `#[parse(from_str)]` or `#[parse(path::to::fn)]` handle foreign types.
//!
//! # Global flags
//!
//! A flag of the root marked `#[flag(global)]` is accepted by every command, before or after the
//! subcommand word. A command that wants its value repeats the flag, with the same names and
//! type; [`check`] reports a flag that does not match the root's. A command that does not repeat
//! it still accepts it, and its help lists it. A root flag that is not global cannot come before
//! a subcommand word, since the root's body does not run then: that is a usage error.
//!
//! ```no_run
//! use defcmd::command;
//!
//! #[command]
//! fn main(#[flag(global, -v)] verbose: bool) {}
//!
//! #[command]
//! fn build(#[flag(global, -v)] verbose: bool) {}
//! ```
//!
//! # Return values and exit codes
//!
//! A command returns `()`, [`std::process::ExitCode`], or a `Result` of either with an error
//! that is `Display` (see [`Output`]). `Err(e)` prints `Error: {e:#}` and exits 1. A usage error
//! prints an `Error:` line and a `help:` line to stderr and exits 2. `--help` and `--version`
//! exit 0.
//!
//! # Testing
//!
//! [`try_run`] and [`try_run_with`] run the registered commands from an explicit argument list
//! and return the exit code. [`check`] validates the command tree and belongs in one `#[test]`.
//! Every command other than `main` stays callable under its own name.
//!
//! # Examples
//!
//! The `defcmd/examples` directory of the repository holds runnable documentation:
//!
//! | Example | Shows |
//! |---|---|
//! | `hello.rs` | the smallest possible command |
//! | `flags_and_arguments.rs` | parameters, flags, parsing rules, alias |
//! | `values.rs` | value types, enums, `FromStr` types, custom parsers |
//! | `nushell_style.rs` | a subcommand tree from names |
//! | `errors.rs` | return types, errors, exit codes |
#![warn(missing_docs)]

// Lets generated code and in-crate tests refer to this crate as `::defcmd`.
extern crate self as defcmd;

mod help;
mod output;
mod parse;
mod run;
mod spec;
#[cfg(test)]
mod test_util;
mod tree;
mod validate;
mod value;

pub use defcmd_macros::{FromArg, command};
pub use output::Output;
pub use parse::{Parsed, UsageError};
pub use run::{try_run, try_run_with};
pub use spec::{CommandSpec, Location, ParamKind, ParamSpec, Registration};
pub use validate::check;
pub use value::{ArgError, FromArg, FromStrArg};

/// Items used by generated code. Not part of the public API.
#[doc(hidden)]
pub mod __private {
    pub use inventory;

    use crate::ArgError;
    use std::ffi::OsStr;
    use std::fmt::Display;
    use std::str::FromStr;

    pub use crate::value::default_type_name;

    /// `#[parse(from_str)]`: parse with `FromStr`.
    pub fn from_str_arg<T>(arg: &OsStr) -> Result<T, ArgError>
    where
        T: FromStr,
        T::Err: Display,
    {
        <crate::FromStrArg<T> as crate::FromArg>::from_arg(arg).map(|wrapped| wrapped.0)
    }

    /// `#[parse(path::to::fn)]`: turns the error of a user function into an `ArgError`.
    pub fn adapt<T, E: Display>(
        f: impl Fn(&OsStr) -> Result<T, E>,
    ) -> impl Fn(&OsStr) -> Result<T, ArgError> {
        move |arg| f(arg).map_err(ArgError::new)
    }

    /// No fixed spellings: the `possible_values` of a parameter with `#[parse(..)]`.
    pub fn no_values() -> &'static [&'static str] {
        &[]
    }

    /// The default of a parameter with `#[parse(..)]` is shown with `Display`, bare.
    pub fn display_parsed<T: Display>(value: &T) -> Option<String> {
        Some(value.to_string())
    }

    /// The body of the generated `fn main`. `bin_name` is `option_env!("CARGO_BIN_NAME")` of the
    /// user's crate.
    pub fn main(bin_name: Option<&'static str>) -> std::process::ExitCode {
        crate::run::main_with_bin_name(bin_name)
    }
}

#[cfg(test)]
mod tests {
    use crate::command;

    #[command]
    fn plumbing(x: String) -> Result<(), String> {
        if x.is_empty() { Err(x) } else { Ok(()) }
    }

    #[test]
    fn command_attribute_leaves_function_callable() {
        assert_eq!(plumbing("a".into()), Ok(()));
        assert_eq!(plumbing(String::new()), Err(String::new()));
    }
}
