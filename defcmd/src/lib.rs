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
pub use parse::{Outcome, Parsed, UsageError, parse};
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
