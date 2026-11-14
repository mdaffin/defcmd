//! Value parsing: the `FromArg` trait, `ArgError` and the built-in impls.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::PathBuf;

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
}

impl FromArg for OsString {
    fn from_arg(arg: &OsStr) -> Result<Self, ArgError> {
        Ok(arg.to_owned())
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
}
