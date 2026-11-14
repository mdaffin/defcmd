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

/// One `Subcommands:` line: the full command line and the first line of the doc, if any.
fn subcommand_entry(spec: &CommandSpec, program: &str) -> String {
    let head = command_line(spec, program);
    match spec.doc.lines().next() {
        Some(first) if !first.is_empty() => format!("  {head} - {first}"),
        _ => format!("  {head}"),
    }
}

/// The full `--help` text, ending in a newline. Parts that have nothing to say are left out.
/// `descendants` are all commands below `spec`, already sorted by path.
pub fn render_help(spec: &CommandSpec, program: &str, descendants: &[&CommandSpec]) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !spec.doc.is_empty() {
        parts.push(spec.doc.to_string());
    }
    parts.push(format!("Usage:\n  > {}", render_usage(spec, program)));

    if !descendants.is_empty() {
        let lines: Vec<String> = descendants
            .iter()
            .map(|d| subcommand_entry(d, program))
            .collect();
        parts.push(format!("Subcommands:\n{}", lines.join("\n")));
    }

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

/// A usage error as printed to stderr: an `Error:` line, then an indented `help:` line with the
/// usage and a pointer to `--help`. Ends in a newline.
pub fn render_usage_error(spec: &CommandSpec, program: &str, message: &str) -> String {
    format!(
        "Error: {message}\n  help: Usage: {}. Use `{} --help` for more information.\n",
        render_usage(spec, program),
        command_line(spec, program)
    )
}

/// The `--version` output: `<name> <version>`, ending in a newline.
pub fn render_version(program: &str, version: &str) -> String {
    format!("{program} {version}\n")
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
        assert_eq!(render_help(&HELLO, "hello", &[]), expected);
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
        assert_eq!(render_help(&NESTED, "app", &[]), expected);
    }

    #[test]
    fn empty_parts_are_left_out() {
        let expected = "\
Usage:
  > app

Flags:
  -h, --help: Display the help message for this command
";
        assert_eq!(render_help(&BARE, "app", &[]), expected);
    }

    static CHECK: CommandSpec = CommandSpec {
        path: &["check"],
        aliases: &[],
        doc: "Check a file.\n\nMore detail.",
        params: &[],
        version: None,
    };
    static GET_DEEP: CommandSpec = CommandSpec {
        path: &["get", "deep"],
        aliases: &[],
        doc: "",
        params: &[],
        version: None,
    };

    #[test]
    fn subcommands_are_listed_by_full_path_with_the_first_doc_line() {
        let expected = "\
Usage:
  > conf

Subcommands:
  conf check - Check a file.
  conf get deep

Flags:
  -h, --help: Display the help message for this command
";
        assert_eq!(render_help(&BARE, "conf", &[&CHECK, &GET_DEEP]), expected);
    }

    #[test]
    fn usage_error_has_an_error_line_and_a_help_line() {
        // From the `errors.rs` example docs.
        static PARAMS: [ParamSpec; 2] = [positional("file"), positional("key")];
        static GET: CommandSpec = CommandSpec {
            path: &["get"],
            aliases: &[],
            doc: "",
            params: &PARAMS,
            version: None,
        };
        assert_eq!(
            render_usage_error(&GET, "conf", "missing required argument 'key'"),
            "Error: missing required argument 'key'\n  \
             help: Usage: conf get <file> <key>. Use `conf get --help` for more information.\n"
        );
    }

    #[test]
    fn usage_error_for_the_root_uses_the_program_name() {
        assert_eq!(
            render_usage_error(&HELLO, "hello", "unknown flag '--nope'"),
            "Error: unknown flag '--nope'\n  \
             help: Usage: hello {flags} <name>. Use `hello --help` for more information.\n"
        );
    }

    #[test]
    fn version_line() {
        assert_eq!(render_version("hello", "1.0.0"), "hello 1.0.0\n");
    }
}
