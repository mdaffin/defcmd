//! Positionals, flags, switches, optional values, defaults and variadics.
//!
//! ```text
//! $ copy a.txt                      # `to` is optional, defaults to `.`
//! $ copy a.txt b.txt --force -j 8
//! $ copy a.txt b.txt --retries 5 --no-color
//! $ copy --dry-run a.txt b.txt
//! $ copy remove a.txt b.txt c.txt --recursive
//! $ copy rm a.txt b.txt c.txt --recursive
//!
//! # Flag syntax
//! $ copy a.txt --jobs 8             # `--flag value` ...
//! $ copy a.txt --jobs=8             # ... and `--flag=value` are the same
//! $ copy a.txt -j 8                 # short flags take the next argument
//! $ copy a.txt -j8                  # exit 2: no attached values (same for `-j=8`)
//! $ copy remove -rf a.txt b.txt     # bundled short switches: `-r -f`
//! $ copy rm -rf a.txt b.txt         # same, via the alias
//! $ copy a.txt -fj 8                # only the last flag in a bundle may take a value
//! $ copy a.txt -jf 8                # exit 2: `-j` takes a value but is not last
//! $ copy a.txt -j 1 -j 2            # a repeated flag: the last one wins (jobs=2)
//! $ copy a.txt --force=false        # a switch accepts `=true` / `=false`
//! $ copy a.txt --force=yes          # exit 2: bad value for a switch
//! $ copy remove -- -odd-name.txt    # after `--` everything is a positional
//! $ copy rm -- -odd-name.txt        # same, via the alias
//! $ copy -- remove                  # copies a file called `remove` (see Subcommands)
//! $ copy -5                         # exit 2: unknown flag `-5` (use `copy -- -5`)
//! $ copy a.txt --jobs -1            # exit 2: bad value (`-1` is taken as the value)
//! $ copy a.txt --jobs --force       # exit 2: bad value (`--force` is taken as the value)
//! $ copy a.txt --jobs               # exit 2: `--jobs` is missing its value
//!
//! # Repeated, negatable and tri-state flags
//! $ copy a.txt -x '*.tmp' -x '*.bak' # a `Vec` flag collects every occurrence
//! $ copy a.txt --no-color           # `color` defaults to true; `--color` / `--no-color`
//! $ copy a.txt --color=false        # ... also accepts `=true` / `=false`
//! $ copy a.txt --links              # `Option<bool>`: Some(true)
//! $ copy a.txt --no-links           # ... Some(false); `--links=false` is the same
//! $ copy a.txt                      # ... None (neither given)
//! $ copy a.txt --links false        # `false` is not the value: it is `to`, a positional
//! ```
//!
//! Type -> meaning:
//! - `T`                    required positional
//! - `Option<T>`            optional positional (`None` if absent)
//! - `#[default(expr)]`     optional positional, with a value
//! - `Vec<T>` (last)        variadic positionals (zero or more); `#[rest]` is optional
//!
//! A parameter is a positional unless it has `#[flag]`. All flags are optional:
//! - `#[flag] bool`                     switch (`false` if absent)
//! - `#[flag] Option<T>`                value flag (`None` if absent)
//! - `#[flag] #[default(expr)] T`       value flag (the default if absent)
//! - `#[flag] Vec<T>`                   repeatable value flag (empty if absent)
//! - `#[flag] Option<bool>`             tri-state: also accepts `--no-x`
//! - `#[flag] #[default(b)] bool`       negatable switch: also accepts `--no-x`
//! A flag of any other type without a `#[default]` is a compile error.
//!
//! Naming flags:
//! - `#[flag]`              long name from the parameter (`dry_run` -> `--dry-run`)
//! - `#[flag(-f)]`          adds a short name, keeps the derived long name
//! - `#[flag(--jobs)]`      replaces the long name (the Rust name can differ)
//! - `#[flag(--jobs, -j)]`  both
//!
//! Parsing rules:
//! - Flags and positionals may be mixed in any order after the command word.
//! - A flag that takes a value accepts `--flag value` and `--flag=value`; short flags accept
//!   `-j 8` only (no `-j8`, no `-j=8`).
//! - A flag that takes a value always consumes the next argument, whatever it looks like
//!   (`--name -x`, `--offset -5`, `--jobs --force`).
//! - Every switch accepts `--x`, `--x=true` and `--x=false`.
//! - Short switches bundle: `-rf` is `-r -f`. Only the last flag in a bundle may take a value.
//! - Only a lone `-` is a positional among arguments starting with `-`; `-5` is an unknown
//!   flag. Use `--` to pass such arguments as positionals.
//! - A bare `--` ends flag parsing; everything after it is a positional.
//! - A repeated flag: the last one wins; `Vec` flags collect every occurrence.
//!
//! Subcommands (see `nushell_style.rs` for the full rules):
//! - `fn main` is the root; every other `#[command]` is a subcommand of it, named after the
//!   function (`#[command]` on `fn remove` gives `copy remove`).
//! - `#[command(alias = rm)]` adds another name for the same subcommand.
//! - The root here also takes positionals, so `copy remove` is ambiguous between the
//!   subcommand and a file called `remove`. The subcommand wins; use `--` for the file.
//!
//! Values are parsed via `FromArg`, which has impls for the std types used here. See
//! `values.rs` for enums, `FromStr` types and custom parsers.
//!
//! `copy --help` prints (to stdout, exit 0), in nushell's layout:
//!
//! ```text
//! Copy a file.
//!
//! Usage:
//!   > copy {flags} <from> (to)
//!
//! Subcommands:
//!   copy remove, rm - Remove files.
//!
//! Flags:
//!   -h, --help: Display the help message for this command
//!   -V, --version: Print the version
//!   -f, --force: Overwrite the destination if it exists.
//!   --dry-run: Show what would happen without doing it.
//!   -j, --jobs <int>: Number of parallel jobs. (default: 4)
//!   --retries <int>: Retry count (unset means never retry).
//!   --color, --no-color: Coloured output; `--no-color` turns it off. (default: true)
//!   -x, --exclude <string>: Skip files matching a pattern. (repeatable)
//!   --links, --no-links: Follow symlinks (unset means use the default behaviour).
//!
//! Parameters:
//!   from <path>: Source file.
//!   to <path>: Destination; the current directory if omitted. (optional, default: '.')
//! ```
//!
//! `copy remove --help` lists `...files <path>: Files to remove.` under `Parameters:`, and
//! `copy --help` shows only the first line of a command's doc comment in `Subcommands:`.
//!
//! The examples call the binary `copy`; the real name comes from the Cargo target.

use defcmd::command;
use std::path::PathBuf;

/// Copy a file.
#[command]
fn main(
    /// Source file.
    from: PathBuf,
    /// Destination; the current directory if omitted.
    #[default(PathBuf::from("."))]
    to: PathBuf,
    /// Overwrite the destination if it exists.
    #[flag(-f)] // --force, -f
    force: bool,
    /// Show what would happen without doing it.
    #[flag] // --dry-run
    dry_run: bool,
    /// Number of parallel jobs.
    // The Rust name is `parallelism`; the command line says `--jobs`.
    #[flag(--jobs, -j)]
    #[default(4)]
    parallelism: usize,
    /// Retry count (unset means never retry).
    #[flag]
    retries: Option<u32>,
    /// Coloured output; `--no-color` turns it off.
    #[flag] // --color, --no-color
    #[default(true)]
    color: bool,
    /// Skip files matching a pattern.
    #[flag(-x)] // --exclude, -x
    exclude: Vec<String>,
    /// Follow symlinks (unset means use the default behaviour).
    #[flag] // --links, --no-links
    links: Option<bool>,
) {
    println!(
        "copy {from:?} -> {to:?} force={force} dry_run={dry_run} jobs={parallelism} \
         retries={retries:?} color={color} exclude={exclude:?} links={links:?}"
    );
}

/// Remove files.
#[command(alias = rm)]
fn remove(
    /// Files to remove.
    // `#[rest]` may be empty; this loop then does nothing.
    #[rest]
    files: Vec<PathBuf>,
    /// Descend into directories.
    #[flag(-r)] // --recursive, -r
    recursive: bool,
    /// Ignore files that do not exist.
    #[flag(-f)] // --force, -f
    force: bool,
) {
    for f in files {
        println!("remove {f:?} recursive={recursive} force={force}");
    }
}
