//! Helpers for building descriptors by hand in tests.

use crate::spec::{ParamKind, ParamSpec};
use crate::value::FromArg;

pub const fn positional(name: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        kind: ParamKind::Positional,
        long: None,
        short: None,
        doc: "",
        type_name: <String as FromArg>::type_name,
        possible_values: <String as FromArg>::possible_values,
        default_display: None,
    }
}

pub const fn switch(name: &'static str, long: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        kind: ParamKind::Switch,
        long: Some(long),
        short: None,
        doc: "",
        type_name: <bool as FromArg>::type_name,
        possible_values: <bool as FromArg>::possible_values,
        default_display: None,
    }
}

/// `p` with its doc comment set.
pub const fn documented(p: ParamSpec, doc: &'static str) -> ParamSpec {
    ParamSpec { doc, ..p }
}

/// A value flag (`--long <string>`).
pub const fn flag(name: &'static str, long: &'static str) -> ParamSpec {
    ParamSpec {
        kind: ParamKind::Flag,
        type_name: <String as FromArg>::type_name,
        possible_values: <String as FromArg>::possible_values,
        ..switch(name, long)
    }
}

/// `p` with a short name.
pub const fn with_short(p: ParamSpec, short: char) -> ParamSpec {
    ParamSpec {
        short: Some(short),
        ..p
    }
}
