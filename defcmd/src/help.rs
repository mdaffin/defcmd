//! Rendering of help text. Plain text, no colour.

use crate::spec::{CommandSpec, ParamKind, ParamSpec};

/// The program name followed by the command path, e.g. `app conf get`.
pub fn command_line(spec: &CommandSpec, program: &str) -> String {
    let mut words = vec![program];
    words.extend(spec.path);
    words.join(" ")
}

/// The usage line without the `>`: the command path, `{flags}` if there are flags, then the
/// positionals. For example `copy {flags} <from>`. `inherited` are the global flags of the root
/// that the command does not declare itself; they count as flags.
pub fn render_usage(spec: &CommandSpec, program: &str, inherited: &[&ParamSpec]) -> String {
    let mut line = command_line(spec, program);
    if !inherited.is_empty() || spec.params.iter().any(|p| p.kind != ParamKind::Positional) {
        line.push_str(" {flags}");
    }
    for p in spec
        .params
        .iter()
        .filter(|p| p.kind == ParamKind::Positional)
    {
        if p.rest {
            line.push_str(&format!(" ...({})", p.name));
        } else if p.optional {
            line.push_str(&format!(" ({})", p.name));
        } else {
            line.push_str(&format!(" <{}>", p.name));
        }
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

/// `-j, --jobs <int>: Number of jobs. (default: 4)`. Switches have no type; the default is
/// shown only when the parameter has an explicit one. Without a doc the default follows the head
/// directly.
fn flag_entry(p: &ParamSpec) -> String {
    let mut head = String::new();
    if let Some(short) = p.short {
        head.push_str(&format!("-{short}, "));
    }
    head.push_str(&p.display_name());
    if p.negatable {
        head.push_str(&format!(", --no-{}", p.long.unwrap_or(p.name)));
    }
    if p.kind == ParamKind::Flag {
        head.push_str(&format!(" <{}>", (p.type_name)()));
    }
    let default = if p.repeatable {
        Some("(repeatable)".to_string())
    } else {
        p.default_display
            .and_then(|show| show())
            .map(|d| format!("(default: {d})"))
    };
    match (p.doc.is_empty(), default) {
        (_, None) => entry(head, p.doc),
        (true, Some(d)) => format!("  {head} {d}"),
        (false, Some(d)) => entry(head, &format!("{} {d}", p.doc)),
    }
}

/// `from <path>: Source file.`; an optional one ends in `(optional)` or `(optional, default: '.')`.
/// Without a doc the note follows the head directly.
fn parameter_entry(p: &ParamSpec) -> String {
    let dots = if p.rest { "..." } else { "" };
    let head = format!("{dots}{} <{}>", p.name, (p.type_name)());
    if !p.optional {
        return entry(head, p.doc);
    }
    let note = match p.default_display.and_then(|show| show()) {
        Some(d) => format!("(optional, default: {d})"),
        None => "(optional)".to_string(),
    };
    if p.doc.is_empty() {
        format!("  {head} {note}")
    } else {
        entry(head, &format!("{} {note}", p.doc))
    }
}

/// One `Subcommands:` line: the full command line and the first line of the doc, if any.
fn subcommand_entry(spec: &CommandSpec, program: &str) -> String {
    let mut head = command_line(spec, program);
    for alias in spec.aliases {
        head.push_str(&format!(", {alias}"));
    }
    match spec.doc.lines().next() {
        Some(first) if !first.is_empty() => format!("  {head} - {first}"),
        _ => format!("  {head}"),
    }
}

/// The full `--help` text, ending in a newline. Parts that have nothing to say are left out.
/// `descendants` are all commands below `spec`, already sorted by path. `inherited` are the
/// global flags of the root that `spec` does not declare itself: it accepts them all the same.
pub fn render_help(
    spec: &CommandSpec,
    program: &str,
    descendants: &[&CommandSpec],
    inherited: &[&ParamSpec],
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !spec.doc.is_empty() {
        parts.push(spec.doc.to_string());
    }
    parts.push(format!(
        "Usage:\n  > {}",
        render_usage(spec, program, inherited)
    ));

    if !descendants.is_empty() {
        let lines: Vec<String> = descendants
            .iter()
            .map(|d| subcommand_entry(d, program))
            .collect();
        parts.push(format!("Subcommands:\n{}", lines.join("\n")));
    }

    // A flag of the command that declares `-h` or `-V` replaces the generated short form.
    let short_taken = |c: char| {
        spec.params
            .iter()
            .chain(inherited.iter().copied())
            .any(|p| p.kind != ParamKind::Positional && p.short == Some(c))
    };
    let help_head = if short_taken('h') {
        "--help"
    } else {
        "-h, --help"
    };
    let mut flags = vec![entry(
        help_head.to_string(),
        "Display the help message for this command",
    )];
    if spec.version.is_some() {
        let head = if short_taken('V') {
            "--version"
        } else {
            "-V, --version"
        };
        flags.push(entry(head.to_string(), "Print the version"));
    }
    flags.extend(
        spec.params
            .iter()
            .chain(inherited.iter().copied())
            .filter(|p| p.kind != ParamKind::Positional)
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
pub fn render_usage_error(
    spec: &CommandSpec,
    program: &str,
    message: &str,
    inherited: &[&ParamSpec],
) -> String {
    format!(
        "Error: {message}\n  help: Usage: {}. Use `{} --help` for more information.\n",
        render_usage(spec, program, inherited),
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
    use crate::test_util::{
        documented, flag, negatable, optional, positional, repeatable, rest, switch, with_short,
    };
    use crate::value::FromArg;

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

    fn dot() -> Option<String> {
        Some("'.'".to_string())
    }

    // The `copy` block of `examples/flags_and_arguments.rs`: from, (to) with a default, (label).
    static COPY_PARAMS: [ParamSpec; 3] = [
        documented(positional("from"), "Source file."),
        documented(
            ParamSpec {
                default_display: Some(dot),
                ..optional(positional("to"))
            },
            "Destination; the current directory if omitted.",
        ),
        optional(positional("label")),
    ];
    static COPY: CommandSpec = CommandSpec {
        path: &["copy"],
        aliases: &[],
        doc: "",
        params: &COPY_PARAMS,
        version: None,
    };

    #[test]
    fn usage_shows_optional_positionals_in_parentheses() {
        assert_eq!(
            render_usage(&COPY, "app", &[]),
            "app copy <from> (to) (label)"
        );
    }

    #[test]
    fn parameters_show_optional_and_default() {
        let expected = "\
Usage:
  > app copy <from> (to) (label)

Flags:
  -h, --help: Display the help message for this command

Parameters:
  from <string>: Source file.
  to <string>: Destination; the current directory if omitted. (optional, default: '.')
  label <string> (optional)
";
        assert_eq!(render_help(&COPY, "app", &[], &[]), expected);
    }

    static RM_PARAMS: [ParamSpec; 2] = [
        positional("dest"),
        documented(rest(positional("files")), "Files to remove."),
    ];
    static RM: CommandSpec = CommandSpec {
        path: &["rm"],
        aliases: &[],
        doc: "",
        params: &RM_PARAMS,
        version: None,
    };

    #[test]
    fn rest_is_shown_with_dots_in_usage_and_parameters() {
        assert_eq!(render_usage(&RM, "app", &[]), "app rm <dest> ...(files)");
        let expected = "\
Usage:
  > app rm <dest> ...(files)

Flags:
  -h, --help: Display the help message for this command

Parameters:
  dest <string>
  ...files <string>: Files to remove.
";
        assert_eq!(render_help(&RM, "app", &[], &[]), expected);
    }

    // `--verbose` is a global flag of the root that `COPY` does not declare itself.
    static VERBOSE: ParamSpec = documented(
        crate::test_util::global(with_short(switch("verbose", "verbose"), 'v')),
        "Chatty output.",
    );
    static HOST: ParamSpec = crate::test_util::global(with_short(flag("host", "host"), 'h'));

    #[test]
    fn inherited_global_flags_follow_the_commands_own_in_help_and_usage() {
        let expected = "\
Usage:
  > app copy {flags} <from> (to) (label)

Flags:
  -h, --help: Display the help message for this command
  -v, --verbose: Chatty output.

Parameters:
  from <string>: Source file.
  to <string>: Destination; the current directory if omitted. (optional, default: '.')
  label <string> (optional)
";
        assert_eq!(render_help(&COPY, "app", &[], &[&VERBOSE]), expected);
        assert_eq!(
            render_usage(&COPY, "app", &[&VERBOSE]),
            "app copy {flags} <from> (to) (label)"
        );
        assert!(
            render_usage_error(&COPY, "app", "oops", &[&VERBOSE])
                .contains("Usage: app copy {flags} <from>")
        );
    }

    #[test]
    fn an_inherited_short_h_replaces_the_generated_help_form() {
        let help = render_help(&COPY, "app", &[], &[&HOST]);
        assert!(
            help.contains("\n  --help: Display the help message"),
            "{help}"
        );
        assert!(help.contains("\n  -h, --host <string>\n"), "{help}");
    }

    #[test]
    fn usage_for_the_root() {
        assert_eq!(render_usage(&HELLO, "hello", &[]), "hello {flags} <name>");
    }

    #[test]
    fn usage_without_flags_has_no_flags_marker() {
        assert_eq!(render_usage(&NESTED, "app", &[]), "app conf get <key>");
        assert_eq!(render_usage(&BARE, "app", &[]), "app");
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
        assert_eq!(render_help(&HELLO, "hello", &[], &[]), expected);
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
        assert_eq!(render_help(&NESTED, "app", &[], &[]), expected);
    }

    #[test]
    fn empty_parts_are_left_out() {
        let expected = "\
Usage:
  > app

Flags:
  -h, --help: Display the help message for this command
";
        assert_eq!(render_help(&BARE, "app", &[], &[]), expected);
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

    static REMOVE: CommandSpec = CommandSpec {
        path: &["copy", "remove"],
        aliases: &["rm", "del"],
        doc: "Remove files.\n\nMore.",
        params: &[],
        version: None,
    };

    #[test]
    fn subcommand_aliases_follow_the_primary_path() {
        let expected = "\
Usage:
  > app

Subcommands:
  app copy remove, rm, del - Remove files.

Flags:
  -h, --help: Display the help message for this command
";
        let root = CommandSpec {
            path: &[],
            aliases: &[],
            doc: "",
            params: &[],
            version: None,
        };
        assert_eq!(render_help(&root, "app", &[&REMOVE], &[]), expected);
    }

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
        assert_eq!(
            render_help(&BARE, "conf", &[&CHECK, &GET_DEEP], &[]),
            expected
        );
    }

    fn one() -> Option<String> {
        <u8 as FromArg>::display_arg(&1)
    }

    fn none() -> Option<String> {
        None
    }

    const fn int_flag(p: ParamSpec) -> ParamSpec {
        ParamSpec {
            type_name: <u8 as FromArg>::type_name,
            ..p
        }
    }

    // The `todo add` block of `examples/nushell_style.rs`.
    static ADD_PARAMS: [ParamSpec; 2] = [
        documented(positional("title"), "What needs doing."),
        documented(
            ParamSpec {
                default_display: Some(one),
                ..with_short(int_flag(flag("priority", "priority")), 'p')
            },
            "1 (low) to 3 (high).",
        ),
    ];
    static ADD: CommandSpec = CommandSpec {
        path: &["add"],
        aliases: &[],
        doc: "Add an item.",
        params: &ADD_PARAMS,
        version: Some("1.0.0"),
    };

    #[test]
    fn value_flag_help_matches_the_nushell_style_example() {
        let expected = "\
Add an item.

Usage:
  > todo add {flags} <title>

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version
  -p, --priority <int>: 1 (low) to 3 (high). (default: 1)

Parameters:
  title <string>: What needs doing.
";
        assert_eq!(render_help(&ADD, "todo", &[], &[]), expected);
    }

    static MIXED_PARAMS: [ParamSpec; 6] = [
        with_short(
            documented(switch("verbose", "verbose"), "Print extra detail."),
            'v',
        ),
        documented(int_flag(flag("jobs", "jobs")), "Number of jobs."),
        ParamSpec {
            default_display: Some(one),
            ..int_flag(flag("retries", "retries"))
        },
        ParamSpec {
            default_display: Some(none),
            ..documented(flag("name", "name"), "Display name.")
        },
        with_short(flag("out_dir", "out-dir"), 'o'),
        documented(switch("dry_run", "dry-run"), "Do not write."),
    ];
    static MIXED: CommandSpec = CommandSpec {
        path: &[],
        aliases: &[],
        doc: "",
        params: &MIXED_PARAMS,
        version: None,
    };

    #[test]
    fn flags_are_listed_in_declaration_order_with_short_names_types_and_defaults() {
        let expected = "\
Usage:
  > app {flags}

Flags:
  -h, --help: Display the help message for this command
  -v, --verbose: Print extra detail.
  --jobs <int>: Number of jobs.
  --retries <int> (default: 1)
  --name <string>: Display name.
  -o, --out-dir <string>
  --dry-run: Do not write.
";
        assert_eq!(render_help(&MIXED, "app", &[], &[]), expected);
    }

    #[test]
    fn repeatable_flags_are_marked_and_optional_ones_show_no_default() {
        static PARAMS: [ParamSpec; 3] = [
            documented(
                repeatable(with_short(flag("exclude", "exclude"), 'x')),
                "Skip files matching a pattern.",
            ),
            repeatable(flag("include", "include")),
            documented(int_flag(flag("retries", "retries")), "Retry count."),
        ];
        static SPEC: CommandSpec = CommandSpec {
            path: &[],
            aliases: &[],
            doc: "",
            params: &PARAMS,
            version: None,
        };
        let expected = "\
Usage:
  > app {flags}

Flags:
  -h, --help: Display the help message for this command
  -x, --exclude <string>: Skip files matching a pattern. (repeatable)
  --include <string> (repeatable)
  --retries <int>: Retry count.
";
        assert_eq!(render_help(&SPEC, "app", &[], &[]), expected);
    }

    fn truthy() -> Option<String> {
        Some("true".to_string())
    }

    #[test]
    fn negatable_switches_list_both_spellings() {
        static PARAMS: [ParamSpec; 3] = [
            ParamSpec {
                default_display: Some(truthy),
                ..documented(negatable(switch("color", "color")), "Coloured output.")
            },
            documented(negatable(switch("links", "links")), "Follow symlinks."),
            with_short(negatable(switch("quiet", "quiet")), 'q'),
        ];
        static SPEC: CommandSpec = CommandSpec {
            path: &[],
            aliases: &[],
            doc: "",
            params: &PARAMS,
            version: None,
        };
        let expected = "\
Usage:
  > app {flags}

Flags:
  -h, --help: Display the help message for this command
  --color, --no-color: Coloured output. (default: true)
  --links, --no-links: Follow symlinks.
  -q, --quiet, --no-quiet
";
        assert_eq!(render_help(&SPEC, "app", &[], &[]), expected);
    }

    #[test]
    fn only_value_flags_still_get_the_flags_marker() {
        static PARAMS: [ParamSpec; 1] = [flag("jobs", "jobs")];
        static ONLY: CommandSpec = CommandSpec {
            path: &[],
            aliases: &[],
            doc: "",
            params: &PARAMS,
            version: None,
        };
        assert_eq!(render_usage(&ONLY, "app", &[]), "app {flags}");
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
            render_usage_error(&GET, "conf", "missing required argument 'key'", &[]),
            "Error: missing required argument 'key'\n  \
             help: Usage: conf get <file> <key>. Use `conf get --help` for more information.\n"
        );
    }

    #[test]
    fn usage_error_for_the_root_uses_the_program_name() {
        assert_eq!(
            render_usage_error(&HELLO, "hello", "unknown flag '--nope'", &[]),
            "Error: unknown flag '--nope'\n  \
             help: Usage: hello {flags} <name>. Use `hello --help` for more information.\n"
        );
    }

    #[test]
    fn version_line() {
        assert_eq!(render_version("hello", "1.0.0"), "hello 1.0.0\n");
    }
}
