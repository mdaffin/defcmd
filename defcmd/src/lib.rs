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
mod value;

pub use defcmd_macros::command;
pub use output::Output;
pub use parse::{Outcome, Parsed, UsageError, parse};
pub use run::{try_run, try_run_with};
pub use spec::{CommandSpec, ParamKind, ParamSpec, Registration};
pub use value::{ArgError, FromArg};

/// Items used by generated code. Not part of the public API.
#[doc(hidden)]
pub mod __private {
    pub use inventory;

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
