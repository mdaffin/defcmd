//! The descriptor-driven argument parser.
//!
//! It parses one command's own flags up to the first positional, then asks a callback whether
//! that word names a child command. If so it returns [`Outcome::Sub`] and the caller recurses.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::os::unix::ffi::OsStrExt;

use crate::spec::{CommandSpec, ParamKind, ParamSpec};
use crate::value::{ArgError, FromArg};

/// A problem with the arguments the user gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageError {
    message: String,
}

impl UsageError {
    /// Builds the error from any message.
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
    /// Positionals, switches (stored as `true` or `false`) and value flags. A repeated flag collects every value.
    Values(Vec<OsString>),
}

/// The raw, not yet converted, result of parsing one command's arguments.
#[derive(Debug)]
pub struct Parsed {
    pub(crate) spec: &'static CommandSpec,
    pub(crate) raw: Vec<Raw>,
}

/// What parsing the arguments of one command decided.
#[derive(Debug)]
pub(crate) enum Outcome {
    /// Run the command with these values.
    Run(Parsed),
    /// `--help` or `-h` was given.
    Help,
    /// `--version` or `-V` was given.
    Version,
    /// The first positional named child `index` (as returned by the callback); `rest` are the
    /// arguments after that word.
    Sub {
        /// Index of the child.
        index: usize,
        /// The remaining arguments.
        rest: Vec<OsString>,
    },
}

/// Where the value of a flag goes.
#[derive(Debug, Clone, Copy)]
enum Target {
    /// The parameter at this index of the command being parsed.
    Own(usize),
    /// The global flag at this index of the root command's parameters. Its value goes to the
    /// store that every level shares.
    Global(usize),
}

/// One flag named by an argument, before it is applied.
struct Use<'a> {
    found: Option<Target>,
    shown: String,
    inline: Option<&'a OsStr>,
    /// Named by `--no-x`.
    negated: bool,
    /// A value flag in a short bundle that is not the last letter.
    not_last: bool,
}

/// Where the flags named in an argument are found: a global flag of the root first, then one of
/// the command's own.
struct Lookup {
    spec: &'static CommandSpec,
    globals: &'static [ParamSpec],
}

impl Lookup {
    /// The first flag, global before own, that `fits`. An own flag that is itself marked global
    /// only repeats a root flag to read its value, so it is never the target.
    fn find(&self, fits: impl Fn(&ParamSpec) -> bool) -> Option<Target> {
        let fits = |p: &ParamSpec| p.kind != ParamKind::Positional && fits(p);
        (self.globals.iter())
            .position(|p| p.global && fits(p))
            .map(Target::Global)
            .or_else(|| {
                (self.spec.params.iter())
                    .position(|p| !p.global && fits(p))
                    .map(Target::Own)
            })
    }

    /// The flag called `--wanted`; with `negatable_only`, one that also accepts `--no-wanted`.
    fn by_long(&self, wanted: &str, negatable_only: bool) -> Option<Target> {
        self.find(|p| p.long == Some(wanted) && (p.negatable || !negatable_only))
    }

    fn by_short(&self, wanted: char) -> Option<Target> {
        self.find(|p| p.short == Some(wanted))
    }

    fn param_of(&self, t: Target) -> &'static ParamSpec {
        match t {
            Target::Own(i) => &self.spec.params[i],
            Target::Global(i) => &self.globals[i],
        }
    }
}

/// What an argument that starts with `-` (and is not `--`) names.
enum Flagged<'a> {
    /// `--help` or `-h`, not taken by a flag of the command.
    Help,
    /// `--version` or `-V`, when the command has a version and no flag takes the name.
    Version,
    /// One entry per flag named by the argument: a long flag, or each letter of a short bundle.
    /// `shown` is the flag as the user wrote it (without any `=value`).
    Uses(Vec<Use<'a>>),
}

/// Splits `arg`, a flag or a bundle of short flags, into the flags it names.
fn flag_uses<'a>(arg: &'a OsStr, look: &Lookup) -> Flagged<'a> {
    let bytes = arg.as_encoded_bytes();
    let has_version = look.spec.version.is_some();
    let mut uses: Vec<Use> = Vec::new();
    if let Some(body) = bytes.strip_prefix(b"--") {
        let (name, inline) = match body.iter().position(|&b| b == b'=') {
            Some(eq) => (&body[..eq], Some(OsStr::from_bytes(&body[eq + 1..]))),
            None => (body, None),
        };
        let name = String::from_utf8_lossy(name);
        if inline.is_none() && name == "help" {
            return Flagged::Help;
        }
        if inline.is_none() && name == "version" && has_version {
            return Flagged::Version;
        }
        let mut found = look.by_long(&name, false);
        let mut negated = false;
        // `--no-x` is the negation of a negatable `x`, unless a switch is really
        // called `--no-x`. A negation takes no `=value`.
        if found.is_none()
            && inline.is_none()
            && let Some(base) = name.strip_prefix("no-")
        {
            found = look.by_long(base, true);
            negated = found.is_some();
        }
        uses.push(Use {
            found,
            shown: format!("--{name}"),
            inline,
            negated,
            not_last: false,
        });
        return Flagged::Uses(uses);
    }
    // Short flags bundle (`-rf` is `-r -f`) but never take an attached value, so
    // `-j8` and `-j=8` are a value flag that is not last.
    // Lossy text would show U+FFFD for the bad bytes, which cannot be quoted back
    // faithfully, so a flag that is not UTF-8 is shown as such.
    let text = arg
        .to_str()
        .map_or_else(|| "<not valid UTF-8>".into(), std::borrow::Cow::Borrowed);
    // An `=` is not a flag letter: `-j=8` and `-f=false` are unknown as a whole.
    let letters: Vec<char> = if text.contains('=') || arg.to_str().is_none() {
        Vec::new()
    } else {
        text[1..].chars().collect()
    };
    if letters.is_empty() {
        uses.push(Use {
            found: None,
            shown: text.to_string(),
            inline: None,
            negated: false,
            not_last: false,
        });
    }
    for (k, &c) in letters.iter().enumerate() {
        let found = look.by_short(c);
        if found.is_none() && c == 'h' {
            return Flagged::Help;
        }
        if found.is_none() && c == 'V' && has_version {
            return Flagged::Version;
        }
        let not_last = k + 1 < letters.len()
            && found.is_some_and(|t| look.param_of(t).kind == ParamKind::Flag);
        uses.push(Use {
            found,
            shown: format!("-{c}"),
            inline: None,
            negated: false,
            not_last,
        });
    }
    Flagged::Uses(uses)
}

/// Parse `args` (without the command word itself) against `spec`, with no global flags.
#[cfg(test)]
pub(crate) fn parse(
    spec: &'static CommandSpec,
    args: &[OsString],
    child: &dyn Fn(&OsStr) -> Option<usize>,
) -> Result<Outcome, UsageError> {
    parse_with_globals(spec, args, child, &[], &mut [])
}

/// Parse `args` (without the command word itself) against `spec`.
///
/// `child` is asked about the first positional word only, before any `--`.
///
/// `globals` are the root command's parameters. The ones marked `global` are accepted by every
/// command: their values go to `store` (one slot per root parameter, shared by every level
/// parsed on the way to the command that runs), and a parameter of `spec` that is marked `global`
/// reads the value of the root flag with the same long name when parsing is done. A flag of
/// `spec` that is not global cannot be followed by a subcommand word, since it would have no
/// effect.
pub(crate) fn parse_with_globals(
    spec: &'static CommandSpec,
    args: &[OsString],
    child: &dyn Fn(&OsStr) -> Option<usize>,
    globals: &'static [ParamSpec],
    store: &mut [Raw],
) -> Result<Outcome, UsageError> {
    let positionals: Vec<usize> = (0..spec.params.len())
        .filter(|&i| spec.params[i].kind == ParamKind::Positional)
        .collect();
    let mut raw = vec![Raw::Absent; spec.params.len()];
    // The first of this command's own (not global) flags that was applied.
    let mut first_own: Option<String> = None;
    let look = Lookup { spec, globals };
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
                return match (pending, first_own) {
                    (Some(e), _) => Err(e),
                    (None, Some(flag)) => Err(UsageError::new(format!(
                        "flag '{flag}' has no effect before the subcommand '{}'",
                        arg.to_string_lossy()
                    ))),
                    (None, None) => Ok(Outcome::Sub {
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
            let uses = match flag_uses(arg, &look) {
                Flagged::Help => return Ok(Outcome::Help),
                Flagged::Version => return Ok(Outcome::Version),
                Flagged::Uses(uses) => uses,
            };
            for u in uses {
                let Use {
                    found,
                    shown,
                    inline,
                    negated,
                    not_last,
                } = u;
                match found {
                    None => {
                        pending.get_or_insert_with(|| {
                            UsageError::new(format!("unknown flag '{shown}'"))
                        });
                    }
                    Some(_) if not_last => {
                        pending.get_or_insert_with(|| {
                            UsageError::new(format!(
                                "flag '{shown}' takes a value, so it must be last in a bundle"
                            ))
                        });
                    }
                    Some(target) => {
                        let param = look.param_of(target);
                        let slot = match target {
                            Target::Own(p) => &mut raw[p],
                            Target::Global(p) => &mut store[p],
                        };
                        if matches!(target, Target::Own(_)) {
                            first_own.get_or_insert_with(|| shown.clone());
                        }
                        if param.kind == ParamKind::Switch {
                            let value = match (inline, negated) {
                                (_, true) => OsStr::new("false"),
                                (Some(v), false) => v,
                                (None, false) => OsStr::new("true"),
                            };
                            match bool::from_arg(value) {
                                Ok(b) => *slot = Raw::Values(vec![b.to_string().into()]),
                                Err(e) => {
                                    pending.get_or_insert_with(|| {
                                        UsageError::new(format!(
                                            "invalid value '{}' for '{}': {e}",
                                            value.to_string_lossy(),
                                            param.display_name()
                                        ))
                                    });
                                }
                            }
                            continue;
                        }
                        // A value flag takes the next argument verbatim, whatever it looks like.
                        let value = match inline {
                            Some(v) => Some(v.to_owned()),
                            None => args.get(i + 1).cloned().inspect(|_| i += 1),
                        };
                        match value {
                            Some(v) => match slot {
                                Raw::Values(values) if param.repeatable => values.push(v),
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
    // A global flag the command declares gets the value given anywhere on the way here.
    for (i, p) in spec.params.iter().enumerate().filter(|(_, p)| p.global) {
        if let Some(g) = globals.iter().position(|g| p.redeclares(g)) {
            raw[i] = store[g].clone();
        }
    }
    Ok(Outcome::Run(Parsed { spec, raw }))
}

impl Parsed {
    /// The converted value of parameter `index`, or `None` if it was not given. If it was given
    /// more than once, the last value is used.
    pub fn get_opt<T: FromArg>(&self, index: usize) -> Result<Option<T>, UsageError> {
        self.get_opt_with(index, T::from_arg)
    }

    /// Like [`Parsed::get_opt`], converting with `parse` instead of `FromArg` (`#[parse(..)]`).
    pub fn get_opt_with<T>(
        &self,
        index: usize,
        parse: impl Fn(&OsStr) -> Result<T, ArgError>,
    ) -> Result<Option<T>, UsageError> {
        let Raw::Values(values) = &self.raw[index] else {
            return Ok(None);
        };
        let Some(value) = values.last() else {
            return Ok(None);
        };
        self.convert(index, value, parse).map(Some)
    }

    /// Every value of parameter `index`, converted, in the order given; empty if there were none.
    /// For the rest positional.
    pub fn get_all<T: FromArg>(&self, index: usize) -> Result<Vec<T>, UsageError> {
        self.get_all_with(index, T::from_arg)
    }

    /// Like [`Parsed::get_all`], converting with `parse` instead of `FromArg`.
    pub fn get_all_with<T>(
        &self,
        index: usize,
        parse: impl Fn(&OsStr) -> Result<T, ArgError>,
    ) -> Result<Vec<T>, UsageError> {
        let Raw::Values(values) = &self.raw[index] else {
            return Ok(Vec::new());
        };
        values
            .iter()
            .map(|v| self.convert(index, v, &parse))
            .collect()
    }

    fn convert<T>(
        &self,
        index: usize,
        value: &OsStr,
        parse: impl Fn(&OsStr) -> Result<T, ArgError>,
    ) -> Result<T, UsageError> {
        parse(value).map_err(|e| {
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
        self.get_with(index, T::from_arg)
    }

    /// Like [`Parsed::get`], converting with `parse` instead of `FromArg`.
    pub fn get_with<T>(
        &self,
        index: usize,
        parse: impl Fn(&OsStr) -> Result<T, ArgError>,
    ) -> Result<T, UsageError> {
        self.get_opt_with(index, parse)?.ok_or_else(|| {
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
    use std::os::unix::ffi::OsStringExt;

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
    fn a_short_flag_that_is_not_utf8_is_not_shown_lossily() {
        let bad = OsString::from_vec(vec![b'-', 0xff]);
        let e = parse(&COPY, &[bad], &no_child).unwrap_err();
        assert_eq!(e.to_string(), "unknown flag '<not valid UTF-8>'");
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
        let out = parse(&COPY, &args(&["list", "-x", "y"]), &child).unwrap();
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
        // The first `list` is the value of `--jobs`; the second one names the child, which a
        // flag of the command itself cannot precede.
        let e = parse(&BUILD, &args(&["--jobs", "list", "list", "x"]), &child).unwrap_err();
        assert_eq!(
            e.to_string(),
            "flag '--jobs' has no effect before the subcommand 'list'"
        );
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
    fn short_forms_other_than_dash_x_are_rejected() {
        let bundle = "flag '-j' takes a value, so it must be last in a bundle";
        assert_eq!(build_err(&["t", "-j8"]), bundle);
        assert_eq!(build_err(&["t", "-jf"]), bundle);
        assert_eq!(build_err(&["t", "-j=8"]), "unknown flag '-j=8'");
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
        assert_eq!(
            build_err(&["t", "-jobs"]),
            "flag '-j' takes a value, so it must be last in a bundle"
        );
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
    use crate::test_util::{flag, negatable, positional, repeatable, switch, with_short};

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

    // to (positional), color (negatable, -c), links (negatable), plain (switch), no_plain (switch).
    static NEG_PARAMS: [ParamSpec; 5] = [
        positional("to"),
        negatable(with_short(switch("color", "color"), 'c')),
        negatable(switch("links", "links")),
        switch("plain", "plain"),
        switch("no_plain", "no-plain"),
    ];
    static NEG: CommandSpec = CommandSpec {
        path: &["copy"],
        aliases: &[],
        doc: "",
        params: &NEG_PARAMS,
        version: None,
    };

    fn neg(a: &[&str]) -> Result<Parsed, String> {
        match parse(&NEG, &args(a), &|_| None) {
            Ok(Outcome::Run(p)) => Ok(p),
            Ok(other) => panic!("expected Run, got {other:?}"),
            Err(e) => Err(e.to_string()),
        }
    }

    fn state(p: &Parsed, i: usize) -> Option<bool> {
        p.get_opt::<bool>(i).unwrap()
    }

    #[test]
    fn negatable_switch_accepts_no_prefix_and_every_switch_spelling() {
        let at = |a: &[&str]| state(&neg(&[&["x"], a].concat()).unwrap(), 1);
        assert_eq!(at(&[]), None);
        assert_eq!(at(&["--color"]), Some(true));
        assert_eq!(at(&["--no-color"]), Some(false));
        assert_eq!(at(&["--color=true"]), Some(true));
        assert_eq!(at(&["--color=false"]), Some(false));
        assert_eq!(at(&["-c"]), Some(true));
        assert_eq!(at(&["--color", "--no-color"]), Some(false));
        assert_eq!(at(&["--no-color", "--color"]), Some(true));
    }

    #[test]
    fn there_is_no_short_form_of_the_negation() {
        assert_eq!(neg(&["x", "-C"]).unwrap_err(), "unknown flag '-C'");
    }

    #[test]
    fn negation_takes_no_value() {
        assert_eq!(
            neg(&["x", "--no-color=true"]).unwrap_err(),
            "unknown flag '--no-color'"
        );
    }

    #[test]
    fn a_negatable_switch_does_not_take_the_next_argument() {
        // `false` is the next positional, not the switch value.
        assert_eq!(
            neg(&["x", "--links", "false"]).unwrap_err(),
            "unexpected argument 'false'"
        );
    }

    #[test]
    fn plain_switch_has_no_negation_but_a_no_named_switch_is_itself() {
        assert_eq!(
            neg(&["x", "--no-links"]).map(|p| state(&p, 2)),
            Ok(Some(false))
        );
        assert_eq!(
            neg(&["x", "--no-plain"]).map(|p| state(&p, 4)),
            Ok(Some(true))
        );
        assert_eq!(neg(&["x", "--no-plain"]).map(|p| state(&p, 3)), Ok(None));
    }

    #[test]
    fn only_negatable_switches_get_the_prefix() {
        // `plain` is not negatable, and `--no-plain` is its own switch (param 4), not `plain`.
        assert_eq!(
            neg(&["x", "--no-to"]).unwrap_err(),
            "unknown flag '--no-to'"
        );
    }

    // to (positional), -r and -f (switches), -j (value flag).
    static BUN_PARAMS: [ParamSpec; 4] = [
        positional("to"),
        with_short(switch("recursive", "recursive"), 'r'),
        with_short(switch("force", "force"), 'f'),
        with_short(flag("jobs", "jobs"), 'j'),
    ];
    static BUN: CommandSpec = CommandSpec {
        path: &["rm"],
        aliases: &[],
        doc: "",
        params: &BUN_PARAMS,
        version: Some("1.0.0"),
    };

    fn bun(a: &[&str]) -> Result<Outcome, String> {
        parse(&BUN, &args(a), &|_| None).map_err(|e| e.to_string())
    }

    fn bun_run(a: &[&str]) -> Parsed {
        match bun(a) {
            Ok(Outcome::Run(p)) => p,
            other => panic!("expected Run, got {other:?}"),
        }
    }

    const NOT_LAST: &str = "flag '-j' takes a value, so it must be last in a bundle";

    #[test]
    fn short_switches_bundle() {
        let p = bun_run(&["-rf", "x"]);
        assert_eq!(state(&p, 1), Some(true));
        assert_eq!(state(&p, 2), Some(true));
        assert_eq!(p.get::<String>(0).unwrap(), "x");
        let p = bun_run(&["-fr", "x", "-rr"]);
        assert_eq!(state(&p, 1), Some(true));
    }

    #[test]
    fn the_last_flag_of_a_bundle_may_take_the_next_argument() {
        let p = bun_run(&["-fj", "8", "x"]);
        assert_eq!(state(&p, 2), Some(true));
        assert_eq!(p.get::<u8>(3).unwrap(), 8);
        assert_eq!(p.get::<String>(0).unwrap(), "x");
        // Verbatim, like any value flag.
        let p = bun_run(&["x", "-rj", "-5"]);
        assert_eq!(p.get::<i8>(3).unwrap(), -5);
    }

    #[test]
    fn a_value_flag_must_be_last_in_a_bundle() {
        assert_eq!(bun(&["x", "-jf", "8"]).unwrap_err(), NOT_LAST);
        assert_eq!(bun(&["x", "-rjf", "8"]).unwrap_err(), NOT_LAST);
        assert_eq!(bun(&["x", "-j8"]).unwrap_err(), NOT_LAST);
    }

    #[test]
    fn a_bundle_with_a_missing_value_or_unknown_letter() {
        assert_eq!(
            bun(&["x", "-rj"]).unwrap_err(),
            "flag '-j' is missing its value"
        );
        assert_eq!(bun(&["x", "-rq"]).unwrap_err(), "unknown flag '-q'");
        assert_eq!(bun(&["x", "-qr"]).unwrap_err(), "unknown flag '-q'");
        assert_eq!(bun(&["x", "-5"]).unwrap_err(), "unknown flag '-5'");
        assert_eq!(
            bun(&["x", "-r=true"]).unwrap_err(),
            "unknown flag '-r=true'"
        );
    }

    #[test]
    fn help_and_version_may_sit_in_a_bundle() {
        assert!(matches!(bun(&["x", "-rh"]), Ok(Outcome::Help)));
        assert!(matches!(bun(&["x", "-fV"]), Ok(Outcome::Version)));
        // But not after a value flag that swallows the rest, or after `--`.
        assert!(matches!(bun(&["-j", "-h", "x"]), Ok(Outcome::Run(_))));
        assert!(matches!(bun(&["--", "-rh"]), Ok(Outcome::Run(_))));
    }

    #[test]
    fn a_lone_dash_and_double_dash_are_not_bundles() {
        assert_eq!(bun_run(&["-"]).get::<String>(0).unwrap(), "-");
        assert_eq!(bun_run(&["--", "-rf"]).get::<String>(0).unwrap(), "-rf");
        assert_eq!(
            bun_run(&["-r", "--", "-odd"]).get::<String>(0).unwrap(),
            "-odd"
        );
    }

    #[test]
    fn a_non_utf8_flag_value_is_reported_without_the_value() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(b"a\xffb".to_vec());
        let p = match parse(&SPEC, &["a".into(), "--jobs".into(), bad.clone()], &|_| {
            None
        }) {
            Ok(Outcome::Run(p)) => p,
            other => panic!("expected Run, got {other:?}"),
        };
        assert_eq!(
            p.get_opt::<String>(2).unwrap_err().to_string(),
            "invalid value for '--jobs': not valid UTF-8"
        );
        assert_eq!(p.get_opt::<OsString>(2).unwrap(), Some(bad));
    }

    #[test]
    fn negative_numbers_and_flags_are_values_of_a_value_flag() {
        let p = parsed(&["a", "--jobs", "-1"]);
        assert_eq!(p.get_opt::<String>(2).unwrap().as_deref(), Some("-1"));
        assert!(
            p.get_opt::<usize>(2)
                .unwrap_err()
                .to_string()
                .starts_with("invalid value '-1' for '--jobs'")
        );
        let p = parsed(&["a", "--jobs", "--force"]);
        assert_eq!(p.get_opt::<String>(2).unwrap().as_deref(), Some("--force"));
        assert_eq!(p.raw[3], Raw::Absent);
        assert_eq!(err(&["a", "--jobs"]), "flag '--jobs' is missing its value");
        assert_eq!(err(&["a", "-j"]), "flag '-j' is missing its value");
    }

    #[test]
    fn help_beats_other_usage_errors_but_not_a_value_or_the_double_dash() {
        assert!(matches!(
            try_parse(&["--nope", "--help"]),
            Ok(Outcome::Help)
        ));
        assert!(matches!(
            try_parse(&["--help", "--nope"]),
            Ok(Outcome::Help)
        ));
        assert!(try_parse(&["--jobs"]).is_err());
        assert!(matches!(
            try_parse(&["-fh", "--force=yes"]),
            Ok(Outcome::Help)
        ));
        // Taken as the value of a flag, or after `--`, it is not help.
        assert!(matches!(
            try_parse(&["a", "-j", "--help"]),
            Ok(Outcome::Run(_))
        ));
        assert_eq!(err(&["--", "--help", "b"]), "unexpected argument 'b'");
        assert!(matches!(try_parse(&["--", "--help"]), Ok(Outcome::Run(_))));
    }

    #[test]
    fn plain_value_flag_still_keeps_the_last_value() {
        let p = parsed(&["a", "-j", "1", "-j", "2"]);
        assert_eq!(p.get_opt::<u8>(2).unwrap(), Some(2));
    }
}

/// Global flags: declared on the root, accepted at every level, parsed into a shared store.
#[cfg(test)]
mod global_tests {
    use super::*;
    use crate::spec::ParamSpec;
    use crate::test_util::{flag, global, negatable, positional, repeatable, switch, with_short};

    // The root: --verbose/-v and --quiet/-q (switches), --jobs (value flag), --color (negatable),
    // --tag (repeatable value flag), and its own --local, which is not global.
    static ROOT_PARAMS: [ParamSpec; 6] = [
        global(with_short(switch("verbose", "verbose"), 'v')),
        global(with_short(switch("quiet", "quiet"), 'q')),
        global(flag("jobs", "jobs")),
        global(negatable(switch("color", "color"))),
        global(repeatable(flag("tag", "tag"))),
        switch("local", "local"),
    ];
    static ROOT: CommandSpec = CommandSpec {
        path: &[],
        aliases: &[],
        doc: "",
        params: &ROOT_PARAMS,
        version: None,
    };

    // A child that asks for --verbose and --tag, and has its own positional and --all.
    static LIST_PARAMS: [ParamSpec; 4] = [
        positional("name"),
        global(with_short(switch("verbose", "verbose"), 'v')),
        global(repeatable(flag("tag", "tag"))),
        switch("all", "all"),
    ];
    static LIST: CommandSpec = CommandSpec {
        path: &["list"],
        aliases: &[],
        doc: "",
        params: &LIST_PARAMS,
        version: None,
    };

    // A child that declares none of the globals.
    static PLAIN_PARAMS: [ParamSpec; 1] = [positional("name")];
    static PLAIN: CommandSpec = CommandSpec {
        path: &["plain"],
        aliases: &[],
        doc: "",
        params: &PLAIN_PARAMS,
        version: None,
    };

    fn args(a: &[&str]) -> Vec<OsString> {
        a.iter().map(OsString::from).collect()
    }

    fn store() -> Vec<Raw> {
        vec![Raw::Absent; ROOT_PARAMS.len()]
    }

    fn at(
        spec: &'static CommandSpec,
        a: &[&str],
        store: &mut [Raw],
    ) -> Result<Outcome, UsageError> {
        let child = |w: &OsStr| (w == "list").then_some(0).or((w == "plain").then_some(1));
        parse_with_globals(spec, &args(a), &child, &ROOT_PARAMS, store)
    }

    fn values(raw: &Raw) -> Vec<&str> {
        match raw {
            Raw::Absent => vec![],
            Raw::Values(v) => v.iter().map(|s| s.to_str().unwrap()).collect(),
        }
    }

    fn run(spec: &'static CommandSpec, a: &[&str], store: &mut [Raw]) -> Parsed {
        match at(spec, a, store).unwrap() {
            Outcome::Run(p) => p,
            other => panic!("expected Run, got {other:?}"),
        }
    }

    fn err(spec: &'static CommandSpec, a: &[&str], store: &mut [Raw]) -> String {
        at(spec, a, store).unwrap_err().to_string()
    }

    #[test]
    fn the_root_reads_its_own_global_flags() {
        let mut s = store();
        let p = run(&ROOT, &["--verbose", "--jobs", "4", "--local"], &mut s);
        assert_eq!(values(&p.raw[0]), ["true"]);
        assert_eq!(values(&p.raw[2]), ["4"]);
        assert_eq!(values(&p.raw[5]), ["true"]);
        assert_eq!(p.raw[1], Raw::Absent);
    }

    #[test]
    fn a_global_flag_before_the_subcommand_word_reaches_the_child() {
        let mut s = store();
        let Outcome::Sub { index, rest } = at(&ROOT, &["--verbose", "list", "x"], &mut s).unwrap()
        else {
            panic!("expected Sub");
        };
        assert_eq!((index, rest), (0, args(&["x"])));
        assert_eq!(values(&s[0]), ["true"]);
        let p = run(&LIST, &["x"], &mut s);
        assert_eq!(values(&p.raw[0]), ["x"]);
        assert_eq!(values(&p.raw[1]), ["true"]);
        assert_eq!(p.raw[2], Raw::Absent);
    }

    #[test]
    fn a_global_flag_after_the_subcommand_word_reaches_the_child() {
        let mut s = store();
        let p = run(&LIST, &["x", "-v", "--all"], &mut s);
        assert_eq!(values(&p.raw[1]), ["true"]);
        assert_eq!(values(&p.raw[3]), ["true"]);
        assert_eq!(values(&s[0]), ["true"]);
    }

    #[test]
    fn a_value_given_at_both_levels_keeps_the_last() {
        let mut s = store();
        let _ = at(&ROOT, &["--jobs", "1", "plain"], &mut s).unwrap();
        run(&PLAIN, &["x", "--jobs", "2"], &mut s);
        assert_eq!(values(&s[2]), ["2"]);
    }

    #[test]
    fn repeatable_globals_collect_across_levels() {
        let mut s = store();
        let _ = at(&ROOT, &["--tag", "a", "list"], &mut s).unwrap();
        let p = run(&LIST, &["x", "--tag", "b"], &mut s);
        assert_eq!(values(&p.raw[2]), ["a", "b"]);
    }

    #[test]
    fn a_child_that_does_not_declare_a_global_still_accepts_it() {
        let mut s = store();
        let p = run(&PLAIN, &["--quiet", "x", "--jobs=3"], &mut s);
        assert_eq!(values(&p.raw[0]), ["x"]);
        assert_eq!(values(&s[1]), ["true"]);
        assert_eq!(values(&s[2]), ["3"]);
    }

    #[test]
    fn globals_take_the_usual_flag_forms() {
        let mut s = store();
        run(
            &PLAIN,
            &["x", "-vq", "--no-color", "--verbose=false"],
            &mut s,
        );
        assert_eq!(values(&s[0]), ["false"]);
        assert_eq!(values(&s[1]), ["true"]);
        assert_eq!(values(&s[3]), ["false"]);
        assert_eq!(
            err(&PLAIN, &["x", "--jobs"], &mut store()),
            "flag '--jobs' is missing its value"
        );
        assert_eq!(
            err(&PLAIN, &["x", "--verbose=yes"], &mut store()),
            "invalid value 'yes' for '--verbose': expected bool"
        );
    }

    #[test]
    fn a_root_flag_that_is_not_global_cannot_come_before_a_subcommand() {
        assert_eq!(
            err(&ROOT, &["--local", "list"], &mut store()),
            "flag '--local' has no effect before the subcommand 'list'"
        );
        // After the subcommand word it is an unknown flag of the child, as before.
        assert_eq!(
            err(&LIST, &["x", "--local"], &mut store()),
            "unknown flag '--local'"
        );
        // Globals are fine there, and a plain error elsewhere still wins.
        assert!(at(&ROOT, &["-v", "list"], &mut store()).is_ok());
        assert_eq!(
            err(&ROOT, &["--local", "--nope", "list"], &mut store()),
            "unknown flag '--nope'"
        );
    }

    #[test]
    fn a_global_short_h_replaces_the_generated_help_form() {
        static P: [ParamSpec; 1] = [global(with_short(flag("host", "host"), 'h'))];
        static R: CommandSpec = CommandSpec {
            path: &[],
            aliases: &[],
            doc: "",
            params: &P,
            version: None,
        };
        let mut s = vec![Raw::Absent; 1];
        let out = parse_with_globals(&R, &args(&["-h", "x"]), &|_| None, &P, &mut s).unwrap();
        assert!(matches!(out, Outcome::Run(_)));
        assert_eq!(values(&s[0]), ["x"]);
    }
}
