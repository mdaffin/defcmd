//! Value parsing: built-in types, enums, `FromStr` types, custom parsers and `FromArg`.
//!
//! ```text
//! $ fetch https://example.com
//! $ fetch https://example.com --timeout 1.5min --retries 0x10 --mode careful
//! $ fetch https://example.com --bind 127.0.0.1:8080 --cache 50%
//! $ fetch https://example.com -H 'Accept: */*' --header 'X-Id: 7'   # repeated flag
//! $ fetch https://example.com --timeout 30    # exit 2: a duration needs a unit (`30sec`)
//! $ fetch https://example.com --mode slow     # exit 2: bad value; one of: fast, careful
//! $ fetch https://example.com --workers 0     # exit 2: bad value (`NonZeroU32`)
//! $ fetch https://example.com -H nocolon      # exit 2: bad value, from the custom parser
//! ```
//!
//! Every parameter type is parsed by `FromArg`, which takes an `&OsStr`:
//! - Provided: `String`, `PathBuf`, `OsString`, `bool`, `char`, integers, floats, `Duration`
//!   and the `std::net` address types.
//! - Integers take `0x` / `0o` / `0b` prefixes and `_` separators (`--retries 0x10`,
//!   `--limit 1_000`). Durations take a number and a unit, as in nushell (`30sec`, `250ms`).
//! - `#[derive(FromArg)]` on a fieldless enum parses the kebab-case variant names and lists
//!   them in help and in the error message.
//! - There is no blanket impl for `FromStr`. A `FromStr` type is used with
//!   `#[parse(from_str)]` (or by naming `FromStrArg<T>` as the type).
//! - `#[parse(some_fn)]` uses `fn(&OsStr) -> Result<T, E>` with `E: Display`. For `Option<T>`
//!   and `Vec<T>` parameters it parses the inner `T`.
//! - Your own types can implement `FromArg` directly, as `Percent` does below.
//! - Help shows `FromArg::type_name` as `<type>`: `<duration>`, `<fast|careful>` for the enum,
//!   `<percent>` for `Percent` (the lower-cased type name is the default), `<socketaddr>`.
//!   Defaults come from `display_arg`: `--timeout <duration>: ... (default: 30sec)`,
//!   `--mode <fast|careful>: ... (default: 'fast')`. `NonZeroU32` is `#[parse(from_str)]`, so
//!   it needs `Display` to show its default (`(default: 1)`).
//!
//! The examples call the binary `fetch`; the real name comes from the Cargo target.

use defcmd::{ArgError, FromArg, command};
use std::ffi::OsStr;
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::time::Duration;

/// How hard to try. Parsed as `fast` or `careful`.
#[derive(Debug, FromArg)]
enum Mode {
    Fast,
    Careful,
}

/// A whole number from 0 to 100, optionally followed by `%`.
#[derive(Debug)]
struct Percent(u8);

impl FromArg for Percent {
    fn from_arg(arg: &OsStr) -> Result<Self, ArgError> {
        let s = arg
            .to_str()
            .ok_or_else(|| ArgError::new("not valid UTF-8"))?;
        let n: u8 = s
            .strip_suffix('%')
            .unwrap_or(s)
            .parse()
            .map_err(ArgError::new)?;
        if n > 100 {
            return Err(ArgError::new("must be between 0 and 100"));
        }
        Ok(Percent(n))
    }
}

/// Custom parser for `Name: value` header lines.
fn header(arg: &OsStr) -> Result<(String, String), String> {
    let s = arg.to_str().ok_or("not valid UTF-8")?;
    let (name, value) = s.split_once(':').ok_or("expected `Name: value`")?;
    Ok((name.trim().to_string(), value.trim().to_string()))
}

/// Fetch a URL.
#[command]
fn main(
    /// What to fetch.
    url: String,
    /// Give up after this long.
    #[flag]
    #[default(Duration::from_secs(30))]
    timeout: Duration,
    /// How hard to try.
    #[flag]
    #[default(Mode::Fast)]
    mode: Mode,
    /// Retry count.
    #[flag]
    #[default(3)]
    retries: u32,
    /// Number of workers (at least 1).
    #[flag]
    #[parse(from_str)]
    #[default(NonZeroU32::MIN)]
    workers: NonZeroU32,
    /// Local address to bind.
    #[flag]
    bind: Option<SocketAddr>,
    /// How much of the response to cache.
    #[flag]
    cache: Option<Percent>,
    /// Extra request header (repeatable).
    // The Rust name is `headers`; the command line says `--header`.
    #[flag(--header, -H)]
    #[parse(header)]
    headers: Vec<(String, String)>,
) {
    println!(
        "fetch {url} timeout={timeout:?} mode={mode:?} retries={retries} workers={workers} \
         bind={bind:?} cache={cache:?} headers={headers:?}"
    );
}
