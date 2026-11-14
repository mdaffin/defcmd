//! Static descriptors the macro emits for each command. Everything here is `const`-constructible
//! so generated code can place it in a `static`.

use std::io::Write;
use std::process::ExitCode;

use crate::parse::{Parsed, UsageError};

/// What kind of parameter a [`ParamSpec`] describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    /// Matched by position, in declaration order.
    Positional,
    /// A flag without a value: `--x`, `--x=true`, `--x=false`.
    Switch,
    /// A flag that takes a value: `--x value`, `--x=value`, `-x value`.
    Flag,
}

/// One parameter of a command.
#[derive(Debug)]
pub struct ParamSpec {
    /// The Rust parameter name (raw identifiers already stripped).
    pub name: &'static str,
    pub kind: ParamKind,
    /// Long flag name without the dashes. `None` for positionals.
    pub long: Option<&'static str>,
    /// Short flag name without the dash. `None` for positionals and flags without one.
    pub short: Option<char>,
    pub doc: &'static str,
    pub type_name: fn() -> String,
    pub possible_values: fn() -> &'static [&'static str],
    /// Renders the default for help; `None` if the parameter has no explicit default.
    pub default_display: Option<fn() -> Option<String>>,
    /// A positional that may be left out (`Option<T>` or `#[default]`). Always false for flags.
    pub optional: bool,
    /// The variadic positional: takes every remaining positional, zero or more. Must be the last
    /// positional; its type is that of one element.
    pub rest: bool,
    /// A value flag that collects every occurrence (`Vec<T>`): `-x a -x b`.
    pub repeatable: bool,
    /// A switch that also accepts `--no-<long>` (`Option<bool>`, or `bool` with a default).
    pub negatable: bool,
}

impl ParamSpec {
    /// The name used in error messages: `--long` for flags, the bare name for positionals.
    pub fn display_name(&self) -> String {
        match self.long {
            Some(long) => format!("--{long}"),
            None => self.name.to_string(),
        }
    }
}

/// One command (a function marked `#[command]`).
#[derive(Debug)]
pub struct CommandSpec {
    /// The command path, e.g. `["copy", "remove"]`. The root command is `[]`.
    pub path: &'static [&'static str],
    pub aliases: &'static [&'static str],
    pub doc: &'static str,
    pub params: &'static [ParamSpec],
    /// The crate version, if `--version` / `-V` is generated for this command.
    pub version: Option<&'static str>,
}

/// What `#[command]` submits to the registry: the descriptor and the glue that converts the
/// parsed values, calls the user's function and reports its result.
pub struct Registration {
    pub spec: &'static CommandSpec,
    /// Runs the command. `Err` is a usage error found while converting values; the `Ok` code is
    /// the exit code. Anything the command's result needs to report goes to the writer.
    pub run: fn(&Parsed, &mut dyn Write) -> Result<ExitCode, UsageError>,
}

inventory::collect!(Registration);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FromArg;

    static PARAMS: [ParamSpec; 2] = [
        ParamSpec {
            name: "from",
            kind: ParamKind::Positional,
            long: None,
            short: None,
            doc: "Source file.",
            type_name: <String as FromArg>::type_name,
            possible_values: <String as FromArg>::possible_values,
            default_display: None,
            optional: false,
            rest: false,
            repeatable: false,
            negatable: false,
        },
        ParamSpec {
            name: "dry_run",
            kind: ParamKind::Switch,
            long: Some("dry-run"),
            short: None,
            doc: "",
            type_name: <bool as FromArg>::type_name,
            possible_values: <bool as FromArg>::possible_values,
            default_display: None,
            optional: false,
            rest: false,
            repeatable: false,
            negatable: false,
        },
    ];

    static SPEC: CommandSpec = CommandSpec {
        path: &["copy"],
        aliases: &["cp"],
        doc: "Copy a file.",
        params: &PARAMS,
        version: None,
    };

    #[test]
    fn specs_can_be_statics() {
        assert_eq!(SPEC.path, ["copy"]);
        assert_eq!(SPEC.aliases, ["cp"]);
        assert_eq!(SPEC.params.len(), 2);
        assert_eq!((SPEC.params[0].type_name)(), "string");
        assert!((SPEC.params[0].possible_values)().is_empty());
    }

    #[test]
    fn display_name_is_bare_for_positionals_and_dashed_for_flags() {
        assert_eq!(SPEC.params[0].display_name(), "from");
        assert_eq!(SPEC.params[1].display_name(), "--dry-run");
    }
}
