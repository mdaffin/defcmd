//! The descriptor-driven argument parser.
//!
//! It parses one command's own flags up to the first positional, then asks a callback whether
//! that word names a child command. If so it returns [`Outcome::Sub`] and the caller recurses.

use std::ffi::{OsStr, OsString};
use std::fmt;

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
    /// Positionals and switches (a switch stores `true` or `false`). Later: repeated flags.
    Values(Vec<OsString>),
}

/// The raw, not yet converted, result of parsing one command's arguments.
#[derive(Debug)]
#[allow(dead_code)] // read by `Parsed::get` in the next step
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

    for (i, arg) in args.iter().enumerate() {
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
            let text = arg.to_string_lossy();
            if let Some(long) = text.strip_prefix("--") {
                let (name, value) = match long.split_once('=') {
                    Some((n, v)) => (n, Some(v)),
                    None => (long, None),
                };
                if value.is_none() && name == "help" {
                    return Ok(Outcome::Help);
                }
                if value.is_none() && name == "version" && spec.version {
                    return Ok(Outcome::Version);
                }
                let found = spec
                    .params
                    .iter()
                    .position(|p| p.kind == ParamKind::Switch && p.long == Some(name));
                match found {
                    Some(p) => {
                        let value = value.unwrap_or("true");
                        match bool::from_arg(OsStr::new(value)) {
                            Ok(b) => raw[p] = Raw::Values(vec![b.to_string().into()]),
                            Err(e) => {
                                pending.get_or_insert_with(|| {
                                    UsageError::new(format!(
                                        "invalid value '{value}' for '{}': {e}",
                                        spec.params[p].display_name()
                                    ))
                                });
                            }
                        }
                    }
                    None => {
                        pending.get_or_insert_with(|| {
                            UsageError::new(format!("unknown flag '--{name}'"))
                        });
                    }
                }
            } else if text == "-h" {
                return Ok(Outcome::Help);
            } else if text == "-V" && spec.version {
                return Ok(Outcome::Version);
            } else {
                pending.get_or_insert_with(|| UsageError::new(format!("unknown flag '{text}'")));
            }
        }
    }

    if let Some(e) = pending {
        return Err(e);
    }
    if let Some(&p) = positionals.get(taken) {
        return Err(UsageError::new(format!(
            "missing required argument '{}'",
            spec.params[p].name
        )));
    }
    Ok(Outcome::Run(Parsed { spec, raw }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::ParamSpec;

    const fn positional(name: &'static str) -> ParamSpec {
        ParamSpec {
            name,
            kind: ParamKind::Positional,
            long: None,
            doc: "",
            type_name: <String as FromArg>::type_name,
            possible_values: <String as FromArg>::possible_values,
            default_display: None,
        }
    }

    const fn switch(name: &'static str, long: &'static str) -> ParamSpec {
        ParamSpec {
            name,
            kind: ParamKind::Switch,
            long: Some(long),
            doc: "",
            type_name: <bool as FromArg>::type_name,
            possible_values: <bool as FromArg>::possible_values,
            default_display: None,
        }
    }

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
        version: true,
    };
    static NO_VERSION: CommandSpec = CommandSpec {
        path: &["copy"],
        aliases: &[],
        doc: "",
        params: &PARAMS,
        version: false,
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
}
