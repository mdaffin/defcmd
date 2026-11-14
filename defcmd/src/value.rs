//! Value parsing: the `FromArg` trait, `ArgError` and the built-in impls.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

/// An opaque message describing why a value could not be parsed.
///
/// There is deliberately no blanket `From<E: Display>`: it would conflict with the reflexive
/// `From<T> for T` impl.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgError {
    message: String,
}

impl ArgError {
    pub fn new(message: impl fmt::Display) -> Self {
        ArgError {
            message: message.to_string(),
        }
    }
}

impl fmt::Display for ArgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ArgError {}

/// A type that can be parsed from a single command-line argument.
pub trait FromArg: Sized {
    fn from_arg(arg: &OsStr) -> Result<Self, ArgError>;

    /// The accepted spellings, for help and error messages. Empty means "any".
    fn possible_values() -> &'static [&'static str] {
        &[]
    }

    /// The type as shown in help (`<int>`, `<path>`). The default is the last path segment of
    /// the Rust type, lower-cased.
    fn type_name() -> String {
        let full = std::any::type_name::<Self>();
        // Drop generic arguments first, so `a::Vec<b::T>` gives `vec`, not `t>`.
        let base = full.split('<').next().unwrap_or(full);
        base.rsplit("::").next().unwrap_or(base).to_lowercase()
    }

    /// The value as shown for a default in help, or `None` to show no default.
    fn display_arg(&self) -> Option<String> {
        None
    }
}

impl FromArg for String {
    fn from_arg(arg: &OsStr) -> Result<Self, ArgError> {
        arg.to_str()
            .map(str::to_owned)
            .ok_or_else(|| ArgError::new("not valid UTF-8"))
    }

    fn display_arg(&self) -> Option<String> {
        Some(format!("'{self}'"))
    }
}

impl FromArg for OsString {
    fn from_arg(arg: &OsStr) -> Result<Self, ArgError> {
        Ok(arg.to_owned())
    }

    fn type_name() -> String {
        "string".to_string()
    }

    fn display_arg(&self) -> Option<String> {
        Some(format!("'{}'", self.to_string_lossy()))
    }
}

impl FromArg for PathBuf {
    fn from_arg(arg: &OsStr) -> Result<Self, ArgError> {
        Ok(PathBuf::from(arg))
    }

    fn type_name() -> String {
        "path".to_string()
    }

    fn display_arg(&self) -> Option<String> {
        Some(format!("'{}'", self.display()))
    }
}

impl FromArg for bool {
    fn from_arg(arg: &OsStr) -> Result<Self, ArgError> {
        match arg.to_str() {
            Some("true") => Ok(true),
            Some("false") => Ok(false),
            _ => Err(ArgError::new("expected bool")),
        }
    }

    fn display_arg(&self) -> Option<String> {
        Some(self.to_string())
    }
}

/// Integer syntax: decimal with an optional sign, or a lower-case `0x`/`0o`/`0b`
/// prefix with no sign; `_` may appear anywhere among the digits. Returns the radix and the text
/// to hand to `from_str_radix` (underscores removed, sign kept for decimal).
fn int_syntax(text: &str) -> Option<(u32, String)> {
    let (radix, body) = match text.get(..2) {
        Some("0x") => (16, &text[2..]),
        Some("0o") => (8, &text[2..]),
        Some("0b") => (2, &text[2..]),
        _ => (10, text),
    };
    let cleaned: String = body.chars().filter(|&c| c != '_').collect();
    if radix != 10 && !cleaned.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some((radix, cleaned))
}

macro_rules! int_from_arg {
    ($($t:ty),* $(,)?) => {$(
        impl FromArg for $t {
            fn from_arg(arg: &OsStr) -> Result<Self, ArgError> {
                arg.to_str()
                    .and_then(int_syntax)
                    .and_then(|(radix, digits)| <$t>::from_str_radix(&digits, radix).ok())
                    .ok_or_else(|| ArgError::new("expected int"))
            }

            fn type_name() -> String {
                "int".to_string()
            }

            fn display_arg(&self) -> Option<String> {
                Some(self.to_string())
            }
        }
    )*};
}

int_from_arg!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize
);

impl FromArg for char {
    fn from_arg(arg: &OsStr) -> Result<Self, ArgError> {
        let mut chars = arg
            .to_str()
            .ok_or_else(|| ArgError::new("expected a single character"))?
            .chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => Ok(c),
            _ => Err(ArgError::new("expected a single character")),
        }
    }

    fn type_name() -> String {
        "string".to_string()
    }

    fn display_arg(&self) -> Option<String> {
        Some(format!("'{self}'"))
    }
}

macro_rules! float_from_arg {
    ($($t:ty),* $(,)?) => {$(
        impl FromArg for $t {
            fn from_arg(arg: &OsStr) -> Result<Self, ArgError> {
                arg.to_str()
                    .map(|s| s.replace('_', ""))
                    .and_then(|s| s.parse::<$t>().ok())
                    .ok_or_else(|| ArgError::new("expected float"))
            }

            fn type_name() -> String {
                "float".to_string()
            }

            fn display_arg(&self) -> Option<String> {
                Some(self.to_string())
            }
        }
    )*};
}

float_from_arg!(f32, f64);

/// Nanoseconds per unit, largest first, for `Duration` parsing and display.
const DURATION_UNITS: &[(&str, u128)] = &[
    ("wk", 604_800_000_000_000),
    ("day", 86_400_000_000_000),
    ("hr", 3_600_000_000_000),
    ("min", 60_000_000_000),
    ("sec", 1_000_000_000),
    ("ms", 1_000_000),
    ("us", 1_000),
    ("µs", 1_000),
    ("ns", 1),
];

/// Splits `1.5min` into digits `1`, fraction `5` and unit `min`. No sign, no `_`, no exponent.
fn split_duration(text: &str) -> Option<(&str, &str, &str)> {
    let number_end = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(number_end);
    let (whole, fraction) = match number.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (number, ""),
    };
    let digits_only = |s: &str| s.chars().all(|c| c.is_ascii_digit());
    if (whole.is_empty() && fraction.is_empty()) || !digits_only(whole) || !digits_only(fraction) {
        return None;
    }
    Some((whole, fraction, unit))
}

fn parse_duration(text: &str) -> Option<Duration> {
    let (whole, fraction, unit) = split_duration(text)?;
    let unit_ns = DURATION_UNITS.iter().find(|(name, _)| *name == unit)?.1;
    let whole: u128 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    // More than 18 fractional digits is far below a nanosecond for every unit; drop them.
    let fraction = &fraction[..fraction.len().min(18)];
    let fraction_ns = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<u128>().ok()? * unit_ns / 10u128.pow(fraction.len() as u32)
    };
    let total = whole.checked_mul(unit_ns)?.checked_add(fraction_ns)?;
    let secs = u64::try_from(total / 1_000_000_000).ok()?;
    Some(Duration::new(secs, (total % 1_000_000_000) as u32))
}

impl FromArg for Duration {
    fn from_arg(arg: &OsStr) -> Result<Self, ArgError> {
        arg.to_str()
            .and_then(parse_duration)
            .ok_or_else(|| ArgError::new("expected duration, like 30sec or 250ms"))
    }

    fn type_name() -> String {
        "duration".to_string()
    }

    /// The largest unit that shows the value as a whole number: `90sec`, `2min`, `1500ms`.
    fn display_arg(&self) -> Option<String> {
        let total = self.as_nanos();
        if total == 0 {
            return Some("0sec".to_string());
        }
        let (name, unit_ns) = DURATION_UNITS
            .iter()
            .filter(|(name, _)| *name != "µs")
            .find(|(_, unit_ns)| total.is_multiple_of(*unit_ns))
            .copied()
            .unwrap_or(("ns", 1));
        Some(format!("{}{name}", total / unit_ns))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;

    #[test]
    fn integers_accept_the_spec_spellings() {
        let cases: &[(&str, i64)] = &[
            ("0", 0),
            ("42", 42),
            ("+42", 42),
            ("-42", -42),
            ("007", 7),
            ("1_000", 1000),
            ("1_0_0", 100),
            ("0x10", 16),
            ("0xFF_FF", 65535),
            ("0xff", 255),
            ("0o17", 15),
            ("0b101", 5),
            ("0b1_01", 5),
        ];
        for (input, want) in cases {
            assert_eq!(i64::from_arg(os(input)).unwrap(), *want, "input {input:?}");
        }
    }

    #[test]
    fn integers_reject_other_spellings() {
        let bad = [
            "", "-", "+", "_", "5.0", "1e3", "abc", " 1", "1 ", "0X10", "0B1", "0O7", "-0x10",
            "+0x10", "0x", "0x-5", "0x+5", "0b2", "0o8", "0xG", "--1", "+-1", "١",
        ];
        for input in bad {
            let err = i64::from_arg(os(input)).unwrap_err();
            assert_eq!(err.to_string(), "expected int", "input {input:?}");
        }
    }

    #[test]
    fn integers_reject_non_utf8() {
        let err = i32::from_arg(OsStr::from_bytes(b"1\xff")).unwrap_err();
        assert_eq!(err.to_string(), "expected int");
    }

    #[test]
    fn integers_check_their_range() {
        assert_eq!(u8::from_arg(os("255")).unwrap(), 255);
        assert!(u8::from_arg(os("256")).is_err());
        assert!(u8::from_arg(os("-1")).is_err());
        assert_eq!(i8::from_arg(os("-128")).unwrap(), -128);
        assert!(i8::from_arg(os("128")).is_err());
        assert_eq!(u8::from_arg(os("0xff")).unwrap(), 255);
        assert!(u8::from_arg(os("0x100")).is_err());
        assert_eq!(i128::MIN.to_string().parse::<i128>().unwrap(), i128::MIN);
        assert_eq!(
            i128::from_arg(os(&i128::MIN.to_string())).unwrap(),
            i128::MIN
        );
        assert_eq!(
            u128::from_arg(os(&u128::MAX.to_string())).unwrap(),
            u128::MAX
        );
        assert!(u128::from_arg(os("340282366920938463463374607431768211456")).is_err());
        assert_eq!(usize::from_arg(os("7")).unwrap(), 7);
        assert_eq!(isize::from_arg(os("-7")).unwrap(), -7);
        assert_eq!(u16::from_arg(os("65535")).unwrap(), u16::MAX);
        assert_eq!(i16::from_arg(os("-32768")).unwrap(), i16::MIN);
        assert_eq!(u32::from_arg(os("4294967295")).unwrap(), u32::MAX);
        assert_eq!(i32::from_arg(os("-2147483648")).unwrap(), i32::MIN);
        assert_eq!(u64::from_arg(os("18446744073709551615")).unwrap(), u64::MAX);
    }

    #[test]
    fn integers_are_called_int_and_display_bare() {
        assert_eq!(u8::type_name(), "int");
        assert_eq!(i128::type_name(), "int");
        assert_eq!(usize::type_name(), "int");
        assert!(u8::possible_values().is_empty());
        assert_eq!(1u8.display_arg().as_deref(), Some("1"));
        assert_eq!((-5i64).display_arg().as_deref(), Some("-5"));
    }

    fn os(s: &str) -> &OsStr {
        OsStr::new(s)
    }

    #[test]
    fn arg_error_displays_its_message() {
        assert_eq!(ArgError::new("expected int").to_string(), "expected int");
        assert_eq!(ArgError::new(42).to_string(), "42");
    }

    #[test]
    fn string_accepts_utf8() {
        assert_eq!(String::from_arg(os("héllo")).unwrap(), "héllo");
        assert_eq!(String::from_arg(os("")).unwrap(), "");
    }

    #[test]
    fn string_rejects_non_utf8() {
        let err = String::from_arg(OsStr::from_bytes(b"a\xffb")).unwrap_err();
        assert_eq!(err.to_string(), "not valid UTF-8");
    }

    #[test]
    fn os_string_accepts_anything() {
        let raw = OsStr::from_bytes(b"a\xffb");
        assert_eq!(OsString::from_arg(raw).unwrap(), raw);
    }

    #[test]
    fn path_buf_accepts_anything_and_is_called_path() {
        let raw = OsStr::from_bytes(b"a/\xff");
        assert_eq!(PathBuf::from_arg(raw).unwrap(), PathBuf::from(raw));
        assert_eq!(PathBuf::from_arg(os("")).unwrap(), PathBuf::new());
        assert_eq!(PathBuf::type_name(), "path");
        assert!(PathBuf::possible_values().is_empty());
    }

    #[test]
    fn string_os_string_and_bool_defaults_are_shown_like_nushell() {
        assert_eq!("a b".to_string().display_arg().as_deref(), Some("'a b'"));
        assert_eq!(OsString::from("x").display_arg().as_deref(), Some("'x'"));
        assert_eq!(true.display_arg().as_deref(), Some("true"));
        assert_eq!(false.display_arg().as_deref(), Some("false"));
    }

    #[test]
    fn path_buf_default_is_shown_single_quoted() {
        assert_eq!(
            PathBuf::from("a b/c").display_arg().as_deref(),
            Some("'a b/c'")
        );
    }

    #[test]
    fn bool_accepts_exactly_true_and_false() {
        assert!(bool::from_arg(os("true")).unwrap());
        assert!(!bool::from_arg(os("false")).unwrap());
        for bad in ["True", "FALSE", "yes", "1", "0", "", " true"] {
            let err = bool::from_arg(os(bad)).unwrap_err();
            assert_eq!(err.to_string(), "expected bool", "input {bad:?}");
        }
    }

    #[test]
    fn bool_rejects_non_utf8() {
        let err = bool::from_arg(OsStr::from_bytes(b"\xff")).unwrap_err();
        assert_eq!(err.to_string(), "expected bool");
    }

    #[test]
    fn default_type_name_is_lowercased_last_segment() {
        struct MyThing;
        impl FromArg for MyThing {
            fn from_arg(_: &OsStr) -> Result<Self, ArgError> {
                Ok(MyThing)
            }
        }
        assert_eq!(MyThing::type_name(), "mything");
        assert_eq!(String::type_name(), "string");
        assert_eq!(MyThing.display_arg(), None);
        assert!(MyThing::possible_values().is_empty());
    }

    #[test]
    fn floats_accept_rust_syntax_and_underscores() {
        for (input, want) in [
            (".5", 0.5),
            ("5.", 5.0),
            ("1e3", 1000.0),
            ("1E3", 1000.0),
            ("+1.5", 1.5),
            ("-2.5", -2.5),
            ("7", 7.0),
            ("1_000.5", 1000.5),
            ("1_0e1_0", 1e11),
        ] {
            assert_eq!(f64::from_arg(os(input)).unwrap(), want, "input {input:?}");
        }
        assert_eq!(f32::from_arg(os("1.5")).unwrap(), 1.5f32);
        assert_eq!(f64::from_arg(os("inf")).unwrap(), f64::INFINITY);
        assert_eq!(f64::from_arg(os("-inf")).unwrap(), f64::NEG_INFINITY);
        assert!(f64::from_arg(os("nan")).unwrap().is_nan());
    }

    #[test]
    fn floats_reject_other_spellings() {
        for bad in [
            "", "_", "abc", "1.2.3", " 1", "1 ", "--1", "1e", "0x10", "١",
        ] {
            let err = f64::from_arg(os(bad)).unwrap_err();
            assert_eq!(err.to_string(), "expected float", "input {bad:?}");
        }
        let err = f32::from_arg(OsStr::from_bytes(b"1\xff")).unwrap_err();
        assert_eq!(err.to_string(), "expected float");
    }

    #[test]
    fn floats_are_called_float_and_display_bare() {
        assert_eq!(f32::type_name(), "float");
        assert_eq!(f64::type_name(), "float");
        assert_eq!(1.5f64.display_arg().as_deref(), Some("1.5"));
        assert_eq!(2f32.display_arg().as_deref(), Some("2"));
        assert!(f64::possible_values().is_empty());
    }

    #[test]
    fn char_takes_exactly_one_character() {
        assert_eq!(char::from_arg(os("x")).unwrap(), 'x');
        assert_eq!(char::from_arg(os("é")).unwrap(), 'é');
        assert_eq!(char::from_arg(os("-")).unwrap(), '-');
        for bad in ["", "ab", "  "] {
            let err = char::from_arg(os(bad)).unwrap_err();
            assert_eq!(err.to_string(), "expected a single character", "{bad:?}");
        }
        assert!(char::from_arg(OsStr::from_bytes(b"\xff")).is_err());
    }

    #[test]
    fn char_is_a_string_shown_single_quoted() {
        assert_eq!(char::type_name(), "string");
        assert_eq!(','.display_arg().as_deref(), Some("','"));
    }

    #[test]
    fn os_string_is_called_string() {
        assert_eq!(OsString::type_name(), "string");
        assert_eq!(String::type_name(), "string");
    }

    #[test]
    fn os_string_keeps_non_utf8_and_string_rejects_it() {
        let raw = OsStr::from_bytes(b"\xfe\xff");
        assert_eq!(OsString::from_arg(raw).unwrap().as_bytes(), b"\xfe\xff");
        assert!(String::from_arg(raw).is_err());
    }

    #[test]
    fn duration_accepts_number_and_unit() {
        use std::time::Duration as D;
        for (input, want) in [
            ("30sec", D::from_secs(30)),
            ("250ms", D::from_millis(250)),
            ("1.5min", D::from_secs(90)),
            ("1.5hr", D::from_secs(5400)),
            (".5sec", D::from_millis(500)),
            ("5.sec", D::from_secs(5)),
            ("3ns", D::from_nanos(3)),
            ("7us", D::from_micros(7)),
            ("7µs", D::from_micros(7)),
            ("2day", D::from_secs(172_800)),
            ("1wk", D::from_secs(604_800)),
            ("0sec", D::ZERO),
            ("007ms", D::from_millis(7)),
            ("0.000000001sec", D::from_nanos(1)),
        ] {
            assert_eq!(D::from_arg(os(input)).unwrap(), want, "input {input:?}");
        }
    }

    #[test]
    fn duration_rejects_everything_else() {
        for bad in [
            "",
            "30",
            "-30sec",
            "+30sec",
            "sec",
            ".sec",
            "1sec500ms",
            "30SEC",
            "30Sec",
            "30s",
            "30 sec",
            " 30sec",
            "30sec ",
            "1.2.3sec",
            "1e3sec",
            "1_000sec",
            "inf",
            "30secs",
            "99999999999999999999999999999999999999999999wk",
        ] {
            let err = Duration::from_arg(os(bad)).unwrap_err();
            assert_eq!(
                err.to_string(),
                "expected duration, like 30sec or 250ms",
                "input {bad:?}"
            );
        }
        assert!(Duration::from_arg(OsStr::from_bytes(b"1\xffsec")).is_err());
    }

    #[test]
    fn duration_is_called_duration_and_displays_in_unit_syntax() {
        use std::time::Duration as D;
        assert_eq!(Duration::type_name(), "duration");
        for (value, want) in [
            (D::from_secs(30), "30sec"),
            (D::from_secs(90), "90sec"),
            (D::from_secs(120), "2min"),
            (D::from_secs(7200), "2hr"),
            (D::from_secs(86_400), "1day"),
            (D::from_secs(1_209_600), "2wk"),
            (D::from_millis(1500), "1500ms"),
            (D::from_micros(1), "1us"),
            (D::from_nanos(1500), "1500ns"),
            (D::ZERO, "0sec"),
        ] {
            assert_eq!(value.display_arg().as_deref(), Some(want), "{value:?}");
        }
    }

    #[test]
    fn duration_display_round_trips() {
        use std::time::Duration as D;
        for value in [
            D::from_secs(30),
            D::from_secs(90),
            D::from_millis(1234),
            D::from_nanos(1),
            D::from_secs(3 * 604_800 + 5),
            D::new(1_000_000, 999_999_999),
            D::ZERO,
        ] {
            let shown = value.display_arg().unwrap();
            assert_eq!(Duration::from_arg(os(&shown)).unwrap(), value, "{shown}");
        }
    }
}
