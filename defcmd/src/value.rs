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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;

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
