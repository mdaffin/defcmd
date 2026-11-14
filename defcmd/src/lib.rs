// Lets generated code and in-crate tests refer to this crate as `::defcmd`.
extern crate self as defcmd;

mod help;
mod output;
mod parse;
mod run;
mod spec;
#[cfg(test)]
mod test_util;
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
}

#[cfg(test)]
mod tests {
    use crate::command;

    #[command]
    fn plumbing(x: u32) -> u32 {
        x + 1
    }

    #[test]
    fn command_attribute_leaves_function_callable() {
        assert_eq!(plumbing(1), 2);
    }
}
