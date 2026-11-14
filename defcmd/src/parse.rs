//! The descriptor-driven argument parser.
//!
//! It parses one command's own flags up to the first positional, then asks a callback whether
//! that word names a child command. If so it returns [`Outcome::Sub`] and the caller recurses.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::os::unix::ffi::OsStrExt;

use crate::spec::{CommandSpec, ParamKind};
use crate::value::FromArg;

/// A problem with the arguments the user gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageError {
    message: String,
}

impl UsageError {
    pub fn new(message: impl fmt::Display) -> Self {
        UsageError {
            message: message.to_string(),
        }
    }
}

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for UsageError {}

/// The raw values collected for one parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Raw {
    Absent,
    /// Positionals, switches (stored as `true` or `false`) and value flags. Later: repeated flags.
    Values(Vec<OsString>),
}

/// The raw, not yet converted, result of parsing one command's arguments.
#[derive(Debug)]
pub struct Parsed {
    pub(crate) spec: &'static CommandSpec,
    pub(crate) raw: Vec<Raw>,
}

#[derive(Debug)]
pub enum Outcome {
    Run(Parsed),
    Help,
    Version,
    /// The first positional named child `index` (as returned by the callback); `rest` are the
    /// arguments after that word.
    Sub {
        index: usize,
        rest: Vec<OsString>,
    },
}

/// Parse `args` (without the command word itself) against `spec`.
///
/// `child` is asked about the first positional word only, before any `--`.
pub fn parse(
    spec: &'static CommandSpec,
    args: &[OsString],
    child: &dyn Fn(&OsStr) -> Option<usize>,
) -> Result<Outcome, UsageError> {
    let positionals: Vec<usize> = (0..spec.params.len())
        .filter(|&i| spec.params[i].kind == ParamKind::Positional)
        .collect();
    let mut raw = vec![Raw::Absent; spec.params.len()];
    let mut taken = 0;
    // The first error found. Parsing goes on so that a later `--help` can still win.
    let mut pending: Option<UsageError> = None;
    let mut after_double_dash = false;
    let mut seen_positional = false;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let bytes = arg.as_encoded_bytes();
        if after_double_dash || !bytes.starts_with(b"-") || bytes == b"-" {
            if !after_double_dash
                && !seen_positional
                && let Some(index) = child(arg)
            {
                return match pending {
                    Some(e) => Err(e),
                    None => Ok(Outcome::Sub {
                        index,
                        rest: args[i + 1..].to_vec(),
                    }),
                };
            }
            seen_positional = true;
            match positionals.get(taken) {
                // The rest positional keeps taking words and is never moved past.
                Some(&p) if spec.params[p].rest => match &mut raw[p] {
                    Raw::Values(values) => values.push(arg.clone()),
                    absent => *absent = Raw::Values(vec![arg.clone()]),
                },
                Some(&p) => {
                    raw[p] = Raw::Values(vec![arg.clone()]);
                    taken += 1;
                }
                None => {
                    pending.get_or_insert_with(|| {
                        UsageError::new(format!("unexpected argument '{}'", arg.to_string_lossy()))
                    });
                }
            }
        } else if bytes == b"--" {
            after_double_dash = true;
        } else {
            // `shown` is the flag as the user wrote it (without any `=value`), for messages.
            let (found, shown, inline): (Option<usize>, String, Option<&OsStr>) =
                if let Some(body) = bytes.strip_prefix(b"--") {
                    let (name, inline) = match body.iter().position(|&b| b == b'=') {
                        Some(eq) => (&body[..eq], Some(OsStr::from_bytes(&body[eq + 1..]))),
                        None => (body, None),
                    };
                    let name = String::from_utf8_lossy(name);
                    if inline.is_none() && name == "help" {
                        return Ok(Outcome::Help);
                    }
                    if inline.is_none() && name == "version" && spec.version.is_some() {
                        return Ok(Outcome::Version);
                    }
                    let found = spec
                        .params
                        .iter()
                        .position(|p| p.kind != ParamKind::Positional && p.long == Some(&*name));
                    (found, format!("--{name}"), inline)
                } else {
                    let text = arg.to_string_lossy();
                    if text == "-h" {
                        return Ok(Outcome::Help);
                    }
                    if text == "-V" && spec.version.is_some() {
                        return Ok(Outcome::Version);
                    }
                    // Only exactly `-x` is a short flag: no `-x=1`, no `-x1`.
                    let mut chars = text[1..].chars();
                    let found = match (chars.next(), chars.next()) {
                        (Some(c), None) => spec
                            .params
                            .iter()
                            .position(|p| p.kind != ParamKind::Positional && p.short == Some(c)),
                        _ => None,
                    };
                    (found, text.into_owned(), None)
                };
            match found {
                None => {
                    pending
                        .get_or_insert_with(|| UsageError::new(format!("unknown flag '{shown}'")));
                }
                Some(p) if spec.params[p].kind == ParamKind::Switch => {
                    let value = inline.unwrap_or(OsStr::new("true"));
                    match bool::from_arg(value) {
                        Ok(b) => raw[p] = Raw::Values(vec![b.to_string().into()]),
                        Err(e) => {
                            pending.get_or_insert_with(|| {
                                UsageError::new(format!(
                                    "invalid value '{}' for '{}': {e}",
                                    value.to_string_lossy(),
                                    spec.params[p].display_name()
                                ))
                            });
                        }
                    }
                }
                Some(p) => {
                    // A value flag takes the next argument verbatim, whatever it looks like.
                    let value = match inline {
                        Some(v) => Some(v.to_owned()),
                        None => args.get(i + 1).cloned().inspect(|_| i += 1),
                    };
                    match value {
                        Some(v) => match &mut raw[p] {
                            Raw::Values(values) if spec.params[p].repeatable => values.push(v),
                            slot => *slot = Raw::Values(vec![v]),
                        },
                        None => {
                            pending.get_or_insert_with(|| {
                                UsageError::new(format!("flag '{shown}' is missing its value"))
                            });
                        }
                    }
                }
            }
        }
        i += 1;
    }

    if let Some(e) = pending {
        return Err(e);
    }
    // A required positional cannot follow an optional one, so only the next one needs a look.
    if let Some(&p) = positionals.get(taken)
        && !spec.params[p].optional
        && !spec.params[p].rest
    {
        return Err(UsageError::new(format!(
            "missing required argument '{}'",
            spec.params[p].name
        )));
    }
    Ok(Outcome::Run(Parsed { spec, raw }))
}

impl Parsed {
    /// The converted value of parameter `index`, or `None` if it was not given. If it was given
    /// more than once, the last value is used.
    pub fn get_opt<T: FromArg>(&self, index: usize) -> Result<Option<T>, UsageError> {
        let Raw::Values(values) = &self.raw[index] else {
            return Ok(None);
        };
        let Some(value) = values.last() else {
            return Ok(None);
        };
        self.convert(index, value).map(Some)
    }

    /// Every value of parameter `index`, converted, in the order given; empty if there were none.
    /// For the rest positional.
    pub fn get_all<T: FromArg>(&self, index: usize) -> Result<Vec<T>, UsageError> {
        let Raw::Values(values) = &self.raw[index] else {
            return Ok(Vec::new());
        };
        values.iter().map(|v| self.convert(index, v)).collect()
    }

    fn convert<T: FromArg>(&self, index: usize, value: &OsStr) -> Result<T, UsageError> {
        T::from_arg(value).map_err(|e| {
            let name = self.spec.params[index].display_name();
            // A value that is not UTF-8 cannot be quoted back faithfully, so it is left out.
            match value.to_str() {
                Some(text) => UsageError::new(format!("invalid value '{text}' for '{name}': {e}")),
                None => UsageError::new(format!("invalid value for '{name}': {e}")),
            }
        })
    }

    /// Like [`Parsed::get_opt`], but a missing value is a usage error.
    pub fn get<T: FromArg>(&self, index: usize) -> Result<T, UsageError> {
        self.get_opt(index)?.ok_or_else(|| {
            UsageError::new(format!(
                "missing required argument '{}'",
                self.spec.params[index].name
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::ParamSpec;
    use crate::test_util::{flag, optional, positional, rest, switch, with_short};

    // from, to (positionals), force and dry_run (switches).
    static PARAMS: [ParamSpec; 4] = [
        positional("from"),
        switch("force", "force"),
        positional("to"),
        switch("dry_run", "dry-run"),
    ];
    static COPY: CommandSpec = CommandSpec {
        path: &["copy"],
        aliases: &[],
        doc: "",
        params: &PARAMS,
        version: Some("1.2.3"),
    };
    static NO_VERSION: CommandSpec = CommandSpec {
        path: &["copy"],
        aliases: &[],
        doc: "",
        params: &PARAMS,
        version: None,
    };

    fn args(a: &[&str]) -> Vec<OsString> {
        a.iter().map(OsString::from).collect()
    }

    fn no_child(_: &OsStr) -> Option<usize> {
        None
    }

    fn run(spec: &'static CommandSpec, a: &[&str]) -> Result<Outcome, UsageError> {
        parse(spec, &args(a), &no_child)
    }

    fn values(raw: &Raw) -> Vec<&str> {
        match raw {
            Raw::Absent => vec![],
            Raw::Values(v) => v.iter().map(|s| s.to_str().unwrap()).collect(),
        }
    }

    fn parsed(a: &[&str]) -> Parsed {
        match run(&COPY, a).unwrap() {
            Outcome::Run(p) => p,
            other => panic!("expected Run, got {other:?}"),
        }
    }

    fn err(a: &[&str]) -> String {
        run(&COPY, a).unwrap_err().to_string()
    }

    #[test]
    fn positionals_and_absent_switches() {
        let p = parsed(&["a", "b"]);
        assert_eq!(values(&p.raw[0]), ["a"]);
        assert_eq!(p.raw[1], Raw::Absent);
        assert_eq!(values(&p.raw[2]), ["b"]);
        assert_eq!(p.raw[3], Raw::Absent);
    }

    #[test]
    fn flags_mix_with_positionals() {
        let p = parsed(&["--force", "a", "--dry-run", "b"]);
        assert_eq!(values(&p.raw[0]), ["a"]);
        assert_eq!(values(&p.raw[1]), ["true"]);
        assert_eq!(values(&p.raw[2]), ["b"]);
        assert_eq!(values(&p.raw[3]), ["true"]);
    }

    #[test]
    fn switch_accepts_explicit_bool() {
        assert_eq!(
            values(&parsed(&["a", "b", "--force=true"]).raw[1]),
            ["true"]
        );
        assert_eq!(
            values(&parsed(&["a", "b", "--force=false"]).raw[1]),
            ["false"]
        );
    }

    #[test]
    fn switch_rejects_other_values() {
        assert_eq!(
            err(&["a", "b", "--force=yes"]),
            "invalid value 'yes' for '--force': expected bool"
        );
        assert_eq!(
            err(&["a", "b", "--force="]),
            "invalid value '' for '--force': expected bool"
        );
    }

    #[test]
    fn switch_does_not_take_the_next_argument() {
        // `false` is the next positional, not the switch's value.
        assert_eq!(
            err(&["a", "b", "--force", "false"]),
            "unexpected argument 'false'"
        );
    }

    #[test]
    fn repeated_switch_last_wins() {
        let p = parsed(&["a", "b", "--force", "--force=false"]);
        assert_eq!(values(&p.raw[1]), ["false"]);
    }

    #[test]
    fn unknown_flag() {
        assert_eq!(err(&["a", "b", "--nope"]), "unknown flag '--nope'");
        assert_eq!(err(&["a", "b", "--nope=1"]), "unknown flag '--nope'");
        assert_eq!(err(&["a", "b", "-x"]), "unknown flag '-x'");
    }

    #[test]
    fn dash_digit_is_an_unknown_flag() {
        assert_eq!(err(&["-5", "a", "b"]), "unknown flag '-5'");
    }

    #[test]
    fn lone_dash_is_a_positional() {
        let p = parsed(&["-", "b"]);
        assert_eq!(values(&p.raw[0]), ["-"]);
    }

    #[test]
    fn double_dash_makes_the_rest_positional() {
        let p = parsed(&["--", "--force", "-5"]);
        assert_eq!(values(&p.raw[0]), ["--force"]);
        assert_eq!(p.raw[1], Raw::Absent);
        assert_eq!(values(&p.raw[2]), ["-5"]);
    }

    #[test]
    fn only_the_first_double_dash_is_special() {
        let p = parsed(&["--", "--", "x"]);
        assert_eq!(values(&p.raw[0]), ["--"]);
        assert_eq!(values(&p.raw[2]), ["x"]);
    }

    #[test]
    fn missing_positional_names_the_first_missing() {
        assert_eq!(err(&[]), "missing required argument 'from'");
        assert_eq!(err(&["a"]), "missing required argument 'to'");
        assert_eq!(err(&["--force", "a"]), "missing required argument 'to'");
    }

    // from (required), to (optional), force (switch).
    static OPT_PARAMS: [ParamSpec; 3] = [
        positional("from"),
        optional(positional("to")),
        switch("force", "force"),
    ];
    static OPT: CommandSpec = CommandSpec {
        path: &["copy"],
        aliases: &[],
        doc: "",
        params: &OPT_PARAMS,
        version: None,
    };

    fn opt_parsed(a: &[&str]) -> Parsed {
        match run(&OPT, a).unwrap() {
            Outcome::Run(p) => p,
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    fn optional_positional_may_be_left_out() {
        let p = opt_parsed(&["a"]);
        assert_eq!(values(&p.raw[0]), ["a"]);
        assert_eq!(p.raw[1], Raw::Absent);
    }

    #[test]
    fn optional_positional_is_filled_in_declaration_order_with_flags_anywhere() {
        let p = opt_parsed(&["a", "--force", "b"]);
        assert_eq!(values(&p.raw[0]), ["a"]);
        assert_eq!(values(&p.raw[1]), ["b"]);
        assert_eq!(values(&p.raw[2]), ["true"]);
        let p = opt_parsed(&["--force", "a", "b"]);
        assert_eq!(values(&p.raw[1]), ["b"]);
    }

    #[test]
    fn required_positional_before_the_optional_one_is_still_required() {
        assert_eq!(
            run(&OPT, &[]).unwrap_err().to_string(),
            "missing required argument 'from'"
        );
    }

    #[test]
    fn extras_after_the_optional_positional_are_unexpected() {
        assert_eq!(
            run(&OPT, &["a", "b", "c"]).unwrap_err().to_string(),
            "unexpected argument 'c'"
        );
    }

    // dest (required), files (rest), force (switch).
    static REST_PARAMS: [ParamSpec; 3] = [
        positional("dest"),
        rest(positional("files")),
        switch("force", "force"),
    ];
    static REST: CommandSpec = CommandSpec {
        path: &["rm"],
        aliases: &[],
        doc: "",
        params: &REST_PARAMS,
        version: None,
    };

    fn rest_parsed(a: &[&str]) -> Parsed {
        match run(&REST, a).unwrap() {
            Outcome::Run(p) => p,
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    fn rest_takes_zero_or_more() {
        let p = rest_parsed(&["d"]);
        assert_eq!(values(&p.raw[0]), ["d"]);
        assert_eq!(p.raw[1], Raw::Absent);
        let p = rest_parsed(&["d", "a", "--force", "b", "c"]);
        assert_eq!(values(&p.raw[1]), ["a", "b", "c"]);
        assert_eq!(values(&p.raw[2]), ["true"]);
    }

    #[test]
    fn rest_does_not_make_the_earlier_positionals_optional() {
        assert_eq!(
            run(&REST, &[]).unwrap_err().to_string(),
            "missing required argument 'dest'"
        );
    }

    #[test]
    fn everything_after_double_dash_goes_to_rest() {
        let p = rest_parsed(&["d", "--", "-x", "--force", "--"]);
        assert_eq!(values(&p.raw[1]), ["-x", "--force", "--"]);
        assert_eq!(p.raw[2], Raw::Absent);
    }

    #[test]
    fn get_all_converts_every_value() {
        let p = rest_parsed(&["d", "1", "2"]);
        assert_eq!(p.get_all::<u8>(1).unwrap(), [1, 2]);
        assert_eq!(
            rest_parsed(&["d"]).get_all::<u8>(1).unwrap(),
            Vec::<u8>::new()
        );
        let e = rest_parsed(&["d", "1", "x"]).get_all::<u8>(1).unwrap_err();
        assert!(
            e.to_string().starts_with("invalid value 'x' for 'files': "),
            "{e}"
        );
    }

    #[test]
    fn extra_positional() {
        assert_eq!(err(&["a", "b", "extra"]), "unexpected argument 'extra'");
    }

    #[test]
    fn first_error_is_reported() {
        assert_eq!(err(&["--nope", "a", "b", "extra"]), "unknown flag '--nope'");
    }

    #[test]
    fn help_outcome() {
        assert!(matches!(run(&COPY, &["--help"]), Ok(Outcome::Help)));
        assert!(matches!(run(&COPY, &["-h"]), Ok(Outcome::Help)));
        assert!(matches!(
            run(&COPY, &["a", "b", "--help"]),
            Ok(Outcome::Help)
        ));
    }

    #[test]
    fn help_wins_over_usage_errors() {
        assert!(matches!(
            run(&COPY, &["--nope", "--help"]),
            Ok(Outcome::Help)
        ));
        assert!(matches!(
            run(&COPY, &["--help", "--nope"]),
            Ok(Outcome::Help)
        ));
        assert!(matches!(
            run(&COPY, &["a", "b", "extra", "-h"]),
            Ok(Outcome::Help)
        ));
        assert!(matches!(
            run(&COPY, &["--force=bad", "-h"]),
            Ok(Outcome::Help)
        ));
        // Missing positionals are an error too, and help still wins.
        assert!(matches!(run(&COPY, &["-h"]), Ok(Outcome::Help)));
    }

    #[test]
    fn help_after_double_dash_is_a_positional() {
        let p = parsed(&["--", "--help", "-h"]);
        assert_eq!(values(&p.raw[0]), ["--help"]);
        assert_eq!(values(&p.raw[2]), ["-h"]);
        assert_eq!(err(&["--nope", "--", "--help"]), "unknown flag '--nope'");
    }

    #[test]
    fn version_outcome() {
        assert!(matches!(run(&COPY, &["--version"]), Ok(Outcome::Version)));
        assert!(matches!(run(&COPY, &["-V"]), Ok(Outcome::Version)));
        assert!(matches!(
            run(&COPY, &["--nope", "-V"]),
            Ok(Outcome::Version)
        ));
    }

    #[test]
    fn version_is_an_unknown_flag_when_not_generated() {
        let e = run(&NO_VERSION, &["a", "b", "--version"]).unwrap_err();
        assert_eq!(e.to_string(), "unknown flag '--version'");
        let e = run(&NO_VERSION, &["a", "b", "-V"]).unwrap_err();
        assert_eq!(e.to_string(), "unknown flag '-V'");
    }

    #[test]
    fn first_of_help_and_version_wins() {
        assert!(matches!(
            run(&COPY, &["--help", "--version"]),
            Ok(Outcome::Help)
        ));
        assert!(matches!(
            run(&COPY, &["--version", "--help"]),
            Ok(Outcome::Version)
        ));
    }

    #[test]
    fn first_positional_is_offered_to_the_child_callback() {
        let child = |w: &OsStr| (w == "list").then_some(7);
        let out = parse(&COPY, &args(&["--force", "list", "-x", "y"]), &child).unwrap();
        match out {
            Outcome::Sub { index, rest } => {
                assert_eq!(index, 7);
                assert_eq!(rest, args(&["-x", "y"]));
            }
            other => panic!("expected Sub, got {other:?}"),
        }
    }

    #[test]
    fn child_lookup_only_applies_to_the_first_positional() {
        let child = |w: &OsStr| (w == "list").then_some(7);
        // `a` is the first positional and is not a child, so `list` is just the second one.
        let out = parse(&COPY, &args(&["a", "list"]), &child).unwrap();
        assert!(matches!(out, Outcome::Run(_)));
    }

    #[test]
    fn child_lookup_is_skipped_after_double_dash() {
        let child = |w: &OsStr| (w == "list").then_some(7);
        let out = parse(&COPY, &args(&["--", "list", "b"]), &child).unwrap();
        assert!(matches!(out, Outcome::Run(_)));
    }

    #[test]
    fn own_flag_error_beats_child_lookup() {
        let child = |w: &OsStr| (w == "list").then_some(7);
        let e = parse(&COPY, &args(&["--nope", "list"]), &child).unwrap_err();
        assert_eq!(e.to_string(), "unknown flag '--nope'");
    }

    #[test]
    fn help_before_child_word_is_this_commands_help() {
        let child = |w: &OsStr| (w == "list").then_some(7);
        let out = parse(&COPY, &args(&["--help", "list"]), &child).unwrap();
        assert!(matches!(out, Outcome::Help));
    }

    // Value flags and short names: target (positional), jobs (`-j, --jobs <string>`),
    // force (`-f, --force`), name (`--name <string>`).
    static BUILD_PARAMS: [ParamSpec; 4] = [
        positional("target"),
        with_short(flag("jobs", "jobs"), 'j'),
        with_short(switch("force", "force"), 'f'),
        flag("name", "name"),
    ];
    static BUILD: CommandSpec = CommandSpec {
        path: &["build"],
        aliases: &[],
        doc: "",
        params: &BUILD_PARAMS,
        version: Some("1.0.0"),
    };

    fn build(a: &[&str]) -> Result<Outcome, UsageError> {
        parse(&BUILD, &args(a), &no_child)
    }

    fn build_parsed(a: &[&str]) -> Parsed {
        match build(a).unwrap() {
            Outcome::Run(p) => p,
            other => panic!("expected Run, got {other:?}"),
        }
    }

    fn build_err(a: &[&str]) -> String {
        build(a).unwrap_err().to_string()
    }

    #[test]
    fn value_flag_takes_the_next_argument_or_an_inline_value() {
        assert_eq!(values(&build_parsed(&["--jobs", "8", "t"]).raw[1]), ["8"]);
        assert_eq!(values(&build_parsed(&["--jobs=8", "t"]).raw[1]), ["8"]);
        assert_eq!(values(&build_parsed(&["t", "--jobs=a=b"]).raw[1]), ["a=b"]);
        assert_eq!(values(&build_parsed(&["--jobs=", "t"]).raw[1]), [""]);
        assert_eq!(build_parsed(&["t"]).raw[1], Raw::Absent);
    }

    #[test]
    fn short_value_flag_takes_the_next_argument() {
        let p = build_parsed(&["-j", "8", "t"]);
        assert_eq!(values(&p.raw[1]), ["8"]);
        assert_eq!(values(&p.raw[0]), ["t"]);
    }

    #[test]
    fn value_flag_consumes_the_next_argument_verbatim() {
        assert_eq!(
            values(&build_parsed(&["--jobs", "--force", "t"]).raw[1]),
            ["--force"]
        );
        assert_eq!(
            build_parsed(&["--jobs", "--force", "t"]).raw[2],
            Raw::Absent
        );
        assert_eq!(values(&build_parsed(&["--name", "-x", "t"]).raw[3]), ["-x"]);
        assert_eq!(values(&build_parsed(&["t", "--jobs", "-5"]).raw[1]), ["-5"]);
        assert_eq!(values(&build_parsed(&["-j", "-", "t"]).raw[1]), ["-"]);
        assert_eq!(values(&build_parsed(&["--jobs", "--", "t"]).raw[1]), ["--"]);
    }

    #[test]
    fn value_flag_consumes_help_and_the_child_word() {
        assert!(matches!(
            build(&["--jobs", "--help", "t"]),
            Ok(Outcome::Run(_))
        ));
        let child = |w: &OsStr| (w == "list").then_some(7);
        let out = parse(&BUILD, &args(&["--jobs", "list", "list", "x"]), &child).unwrap();
        assert!(matches!(out, Outcome::Sub { index: 7, .. }), "{out:?}");
    }

    #[test]
    fn value_flag_without_a_value_is_an_error() {
        assert_eq!(
            build_err(&["t", "--jobs"]),
            "flag '--jobs' is missing its value"
        );
        assert_eq!(build_err(&["t", "-j"]), "flag '-j' is missing its value");
        assert_eq!(build_err(&["--name"]), "flag '--name' is missing its value");
    }

    #[test]
    fn help_still_wins_over_a_missing_value() {
        assert!(matches!(build(&["--jobs", "-h", "t"]), Ok(Outcome::Run(_))));
        assert!(matches!(build(&["t", "--jobs=1", "-h"]), Ok(Outcome::Help)));
        assert!(matches!(
            build(&["-j", "--help", "--help"]),
            Ok(Outcome::Help)
        ));
    }

    #[test]
    fn short_forms_other_than_dash_x_are_unknown_flags() {
        assert_eq!(build_err(&["t", "-j8"]), "unknown flag '-j8'");
        assert_eq!(build_err(&["t", "-j=8"]), "unknown flag '-j=8'");
        assert_eq!(build_err(&["t", "-jf"]), "unknown flag '-jf'");
        assert_eq!(build_err(&["t", "-ff"]), "unknown flag '-ff'");
        assert_eq!(build_err(&["t", "-x"]), "unknown flag '-x'");
        assert_eq!(build_err(&["t", "-J", "1"]), "unknown flag '-J'");
    }

    #[test]
    fn short_switch_sets_true_and_takes_no_value() {
        let p = build_parsed(&["-f", "t"]);
        assert_eq!(values(&p.raw[2]), ["true"]);
        assert_eq!(values(&p.raw[0]), ["t"]);
        assert_eq!(build_err(&["-f=false", "t"]), "unknown flag '-f=false'");
    }

    #[test]
    fn short_name_is_not_a_long_name() {
        assert_eq!(build_err(&["t", "--j", "1"]), "unknown flag '--j'");
        assert_eq!(build_err(&["t", "--f"]), "unknown flag '--f'");
        assert_eq!(build_err(&["t", "-jobs"]), "unknown flag '-jobs'");
    }

    #[test]
    fn long_name_of_a_value_flag_does_not_match_a_switch_with_another_name() {
        assert_eq!(build_err(&["t", "--force-it"]), "unknown flag '--force-it'");
        assert_eq!(build_err(&["t", "--jobs-x=1"]), "unknown flag '--jobs-x'");
    }

    #[test]
    fn last_occurrence_of_a_value_flag_wins() {
        let p = build_parsed(&["--jobs", "1", "-j", "2", "--jobs=3", "t"]);
        assert_eq!(p.get::<String>(1).unwrap(), "3");
    }

    #[test]
    fn value_flags_mix_with_positionals() {
        let p = build_parsed(&["--jobs", "4", "t", "--name=x", "-f"]);
        assert_eq!(values(&p.raw[0]), ["t"]);
        assert_eq!(values(&p.raw[1]), ["4"]);
        assert_eq!(values(&p.raw[2]), ["true"]);
        assert_eq!(values(&p.raw[3]), ["x"]);
    }

    #[test]
    fn value_after_double_dash_is_a_positional() {
        assert_eq!(build_err(&["--", "--jobs", "1"]), "unexpected argument '1'");
        assert_eq!(values(&build_parsed(&["--", "--jobs"]).raw[0]), ["--jobs"]);
    }

    #[test]
    fn bad_value_is_reported_against_the_flag() {
        let p = build_parsed(&["-j", "x", "t"]);
        assert_eq!(
            p.get::<bool>(1).unwrap_err().to_string(),
            "invalid value 'x' for '--jobs': expected bool"
        );
    }

    #[test]
    fn non_utf8_inline_value_is_kept() {
        use std::os::unix::ffi::OsStringExt;
        let arg = OsString::from_vec(b"--name=a\xffb".to_vec());
        let p = match parse(&BUILD, &[arg, "t".into()], &no_child).unwrap() {
            Outcome::Run(p) => p,
            other => panic!("expected Run, got {other:?}"),
        };
        assert_eq!(
            p.get::<OsString>(3).unwrap(),
            OsString::from_vec(b"a\xffb".to_vec())
        );
    }

    #[test]
    fn own_value_flag_error_beats_child_lookup() {
        let child = |w: &OsStr| (w == "list").then_some(7);
        let e = parse(&BUILD, &args(&["-j", "list"]), &child).unwrap_err();
        assert_eq!(e.to_string(), "missing required argument 'target'");
    }

    fn raw_parsed(a: &[&str]) -> Parsed {
        match parse(&COPY, &args(a), &no_child).unwrap() {
            Outcome::Run(p) => p,
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    fn get_converts_strings_and_bools() {
        let p = raw_parsed(&["a", "b", "--force"]);
        assert_eq!(p.get::<String>(0).unwrap(), "a");
        assert!(p.get::<bool>(1).unwrap());
        assert_eq!(p.get::<String>(2).unwrap(), "b");
    }

    #[test]
    fn get_opt_is_none_when_absent() {
        let p = raw_parsed(&["a", "b"]);
        assert_eq!(p.get_opt::<bool>(1).unwrap(), None);
        assert_eq!(p.get_opt::<String>(0).unwrap(), Some("a".to_string()));
    }

    #[test]
    fn get_on_an_absent_value_is_missing() {
        let p = raw_parsed(&["a", "b"]);
        assert_eq!(
            p.get::<bool>(3).unwrap_err().to_string(),
            "missing required argument 'dry_run'"
        );
    }

    #[test]
    fn bad_positional_value_names_the_parameter() {
        let p = raw_parsed(&["a", "maybe"]);
        assert_eq!(
            p.get::<bool>(2).unwrap_err().to_string(),
            "invalid value 'maybe' for 'to': expected bool"
        );
    }

    #[test]
    fn bad_flag_value_names_the_flag() {
        // `--dry-run` was validated as a bool, so ask for a different type to force an error.
        struct Never;
        impl FromArg for Never {
            fn from_arg(_: &OsStr) -> Result<Self, crate::ArgError> {
                Err(crate::ArgError::new("expected never"))
            }
        }
        let p = raw_parsed(&["a", "b", "--dry-run"]);
        assert_eq!(
            p.get::<Never>(3).err().unwrap().to_string(),
            "invalid value 'true' for '--dry-run': expected never"
        );
    }

    #[test]
    fn non_utf8_value_omits_the_value() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(b"a\xffb".to_vec());
        let p = match parse(&COPY, &[bad, "b".into()], &no_child).unwrap() {
            Outcome::Run(p) => p,
            other => panic!("expected Run, got {other:?}"),
        };
        assert_eq!(
            p.get::<String>(0).unwrap_err().to_string(),
            "invalid value for 'from': not valid UTF-8"
        );
        // An OsString does not care.
        assert!(p.get::<OsString>(0).is_ok());
    }
}

#[cfg(test)]
mod flag_form_tests {
    use super::*;
    use crate::spec::ParamSpec;
    use crate::test_util::{flag, positional, repeatable, switch, with_short};

    // to (positional), exclude (repeatable value flag, -x), jobs (value flag, -j), force (-f).
    static PARAMS: [ParamSpec; 4] = [
        positional("to"),
        repeatable(with_short(flag("exclude", "exclude"), 'x')),
        with_short(flag("jobs", "jobs"), 'j'),
        with_short(switch("force", "force"), 'f'),
    ];
    static SPEC: CommandSpec = CommandSpec {
        path: &["copy"],
        aliases: &[],
        doc: "",
        params: &PARAMS,
        version: Some("1.0.0"),
    };

    fn args(a: &[&str]) -> Vec<OsString> {
        a.iter().map(OsString::from).collect()
    }

    fn try_parse(a: &[&str]) -> Result<Outcome, UsageError> {
        parse(&SPEC, &args(a), &|_| None)
    }

    fn parsed(a: &[&str]) -> Parsed {
        match try_parse(a).unwrap() {
            Outcome::Run(p) => p,
            other => panic!("expected Run, got {other:?}"),
        }
    }

    fn err(a: &[&str]) -> String {
        try_parse(a).unwrap_err().to_string()
    }

    #[test]
    fn repeatable_flag_collects_every_occurrence_in_order() {
        let p = parsed(&["a", "-x", "*.tmp", "--exclude", "*.bak", "--exclude=c"]);
        assert_eq!(
            p.get_all::<String>(1).unwrap(),
            ["*.tmp", "*.bak", "c"].map(String::from)
        );
    }

    #[test]
    fn repeatable_flag_never_takes_several_values_at_once() {
        // `b` is the next positional, not a second value.
        assert_eq!(err(&["a", "-x", "p", "b"]), "unexpected argument 'b'");
    }

    #[test]
    fn absent_repeatable_flag_is_empty() {
        assert!(parsed(&["a"]).get_all::<String>(1).unwrap().is_empty());
    }

    #[test]
    fn plain_value_flag_still_keeps_the_last_value() {
        let p = parsed(&["a", "-j", "1", "-j", "2"]);
        assert_eq!(p.get_opt::<u8>(2).unwrap(), Some(2));
    }
}
