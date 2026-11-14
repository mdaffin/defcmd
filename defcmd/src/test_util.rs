//! Helpers for building descriptors by hand in tests.

use crate::spec::{ParamKind, ParamSpec};
use crate::value::FromArg;

pub const fn positional(name: &'static str) -> ParamSpec {
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

pub const fn switch(name: &'static str, long: &'static str) -> ParamSpec {
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
