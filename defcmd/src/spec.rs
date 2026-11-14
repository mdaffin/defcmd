//! Static descriptors the macro emits for each command. Everything here is `const`-constructible
//! so generated code can place it in a `static`.

/// What kind of parameter a [`ParamSpec`] describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    /// Matched by position, in declaration order.
    Positional,
    /// A flag without a value: `--x`, `--x=true`, `--x=false`.
    Switch,
}

/// One parameter of a command.
#[derive(Debug)]
pub struct ParamSpec {
    /// The Rust parameter name (raw identifiers already stripped).
    pub name: &'static str,
    pub kind: ParamKind,
    /// Long flag name without the dashes. `None` for positionals.
    pub long: Option<&'static str>,
    pub doc: &'static str,
    pub type_name: fn() -> String,
    pub possible_values: fn() -> &'static [&'static str],
    /// Renders the default for help; `None` if the parameter has no explicit default.
    pub default_display: Option<fn() -> Option<String>>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FromArg;

    static PARAMS: [ParamSpec; 2] = [
        ParamSpec {
            name: "from",
            kind: ParamKind::Positional,
            long: None,
            doc: "Source file.",
            type_name: <String as FromArg>::type_name,
            possible_values: <String as FromArg>::possible_values,
            default_display: None,
        },
        ParamSpec {
            name: "dry_run",
            kind: ParamKind::Switch,
            long: Some("dry-run"),
            doc: "",
            type_name: <bool as FromArg>::type_name,
            possible_values: <bool as FromArg>::possible_values,
            default_display: None,
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
