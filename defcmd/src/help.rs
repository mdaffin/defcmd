//! Rendering of help text. Plain text, no colour.

use crate::spec::{CommandSpec, ParamKind, ParamSpec};

/// The program name followed by the command path, e.g. `app conf get`.
pub fn command_line(spec: &CommandSpec, program: &str) -> String {
    let mut words = vec![program];
    words.extend(spec.path);
    words.join(" ")
}

/// The usage line without the `>`: the command path, `{flags}` if there are flags, then the
/// positionals. For example `copy {flags} <from>`.
pub fn render_usage(spec: &CommandSpec, program: &str) -> String {
    let mut line = command_line(spec, program);
    if spec.params.iter().any(|p| p.kind == ParamKind::Switch) {
        line.push_str(" {flags}");
    }
    for p in spec
        .params
        .iter()
        .filter(|p| p.kind == ParamKind::Positional)
    {
        line.push_str(&format!(" <{}>", p.name));
    }
    line
}

/// `head: doc`, or just `head` if there is no doc.
fn entry(head: String, doc: &str) -> String {
    if doc.is_empty() {
        format!("  {head}")
    } else {
        format!("  {head}: {doc}")
    }
}

fn flag_entry(p: &ParamSpec) -> String {
    entry(p.display_name(), p.doc)
}

fn parameter_entry(p: &ParamSpec) -> String {
    entry(format!("{} <{}>", p.name, (p.type_name)()), p.doc)
}

/// The full `--help` text, ending in a newline. Parts that have nothing to say are left out.
pub fn render_help(spec: &CommandSpec, program: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !spec.doc.is_empty() {
        parts.push(spec.doc.to_string());
    }
    parts.push(format!("Usage:\n  > {}", render_usage(spec, program)));

    let mut flags = vec![entry(
        "-h, --help".to_string(),
        "Display the help message for this command",
    )];
    if spec.version.is_some() {
        flags.push(entry("-V, --version".to_string(), "Print the version"));
    }
    flags.extend(
        spec.params
            .iter()
            .filter(|p| p.kind == ParamKind::Switch)
            .map(flag_entry),
    );
    parts.push(format!("Flags:\n{}", flags.join("\n")));

    let positionals: Vec<String> = spec
        .params
        .iter()
        .filter(|p| p.kind == ParamKind::Positional)
        .map(parameter_entry)
        .collect();
    if !positionals.is_empty() {
        parts.push(format!("Parameters:\n{}", positionals.join("\n")));
    }

    let mut text = parts.join("\n\n");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::ParamSpec;
    use crate::test_util::{documented, positional, switch};

    static HELLO_PARAMS: [ParamSpec; 2] = [
        documented(positional("name"), "Who to greet."),
        documented(switch("shout", "shout"), "Print in upper case."),
    ];
    static HELLO: CommandSpec = CommandSpec {
        path: &[],
        aliases: &[],
        doc: "Print a greeting.",
        params: &HELLO_PARAMS,
        version: Some("1.0.0"),
    };

    static BARE: CommandSpec = CommandSpec {
        path: &[],
        aliases: &[],
        doc: "",
        params: &[],
        version: None,
    };

    static NESTED_PARAMS: [ParamSpec; 1] = [positional("key")];
    static NESTED: CommandSpec = CommandSpec {
        path: &["conf", "get"],
        aliases: &[],
        doc: "Read a key.\n\nPrints the value of KEY.",
        params: &NESTED_PARAMS,
        version: None,
    };

    #[test]
    fn usage_for_the_root() {
        assert_eq!(render_usage(&HELLO, "hello"), "hello {flags} <name>");
    }

    #[test]
    fn usage_without_flags_has_no_flags_marker() {
        assert_eq!(render_usage(&NESTED, "app"), "app conf get <key>");
        assert_eq!(render_usage(&BARE, "app"), "app");
    }

    #[test]
    fn help_for_the_root() {
        let expected = "\
Print a greeting.

Usage:
  > hello {flags} <name>

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version
  --shout: Print in upper case.

Parameters:
  name <string>: Who to greet.
";
        assert_eq!(render_help(&HELLO, "hello"), expected);
    }

    #[test]
    fn long_description_is_verbatim() {
        let expected = "\
Read a key.

Prints the value of KEY.

Usage:
  > app conf get <key>

Flags:
  -h, --help: Display the help message for this command

Parameters:
  key <string>
";
        assert_eq!(render_help(&NESTED, "app"), expected);
    }

    #[test]
    fn empty_parts_are_left_out() {
        let expected = "\
Usage:
  > app

Flags:
  -h, --help: Display the help message for this command
";
        assert_eq!(render_help(&BARE, "app"), expected);
    }
}
