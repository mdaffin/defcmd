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
        T::from_arg(value).map(Some).map_err(|e| {
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
