# defcmd

A command is a plain Rust function marked `#[command]`, and its signature is the command line.
Parameters, flags, subcommands, help and error output follow [nushell](https://nushell.sh)'s
conventions, with no builder API and no boilerplate. Targets macOS and Linux.

```rust
use defcmd::command;

/// Print a greeting.
#[command]
fn main(
    /// Who to greet.
    name: String,
    /// Print in upper case.
    #[flag]
    shout: bool,
) {
    let msg = format!("Hello, {name}!");
    println!("{}", if shout { msg.to_uppercase() } else { msg });
}
```

```text
$ hello World
Hello, World!
$ hello World --shout
HELLO, WORLD!
$ hello --help
Print a greeting.

Usage:
  > hello {flags} <name>

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version
  --shout: Print in upper case.

Parameters:
  name <string>: Who to greet.
```

The doc comment on a command is its help text; the doc comment on each parameter describes it.

## Features

- **Parameters by signature.** Required, optional, defaulted and variadic positionals, plus
  `#[flag]` switches, value flags, repeatable flags, tri-state switches and negatable switches.
- **Flag naming.** `#[flag]` derives `--long-name` from the Rust name; `#[flag(-f)]`,
  `#[flag(--jobs)]` and `#[flag(--jobs, -j)]` choose the short and long spellings.
- **Value parsing.** Built-in parsing for strings, paths, integers, floats, `bool`, `char`,
  `Duration` and `std::net` addresses (integers take `0x`/`0o`/`0b` and `_` separators; durations
  take a unit as in nushell). `#[derive(FromArg)]` for fieldless enums, `#[parse(from_str)]` for
  any `FromStr` type, `#[parse(fn)]` for a custom parser, and your own `FromArg` impl.
- **Subcommands.** One `#[command]` per function, nested with `#[command("remote add")]`, with
  `#[command(alias = rm)]` for alternate names. The tree is built from name prefixes, so there is
  no list of subcommands and no `parent` to manage. Missing middle nodes become help-only groups.
- **Global flags.** `#[flag(global)]` on the root makes a flag accepted by every command, before or
  after the subcommand word; commands that read it repeat the flag.
- **Exit codes.** A command returns `()`, [`std::process::ExitCode`], or a `Result` with a
  `Display` error. `Err` exits 1, usage errors exit 2, and `--help`/`--version` exit 0.
- **Testing.** `try_run`/`try_run_with` run the registered commands from an explicit argument list
  and return the exit code; `check` validates the whole tree once.

## Parameters and flags

Every parameter is a positional unless it has `#[flag]`. All flags are optional.

| Declaration | Meaning |
|---|---|
| `x: T` | required positional |
| `x: Option<T>` | optional positional |
| `#[default(expr)] x: T` | optional positional with a default |
| `x: Vec<T>` or `#[rest] x: Vec<T>` | variadic positionals, last |
| `#[flag] x: bool` | switch |
| `#[flag] x: Option<T>` | value flag |
| `#[flag] #[default(expr)] x: T` | value flag with a default |
| `#[flag] x: Vec<T>` | repeatable value flag |
| `#[flag] x: Option<bool>` | tri-state switch (also `--no-x`) |
| `#[flag] #[default(true)] x: bool` | negatable switch (also `--no-x`) |
| `#[flag(global)] x: T` | global flag, on the root and repeated on the commands that read it |

```rust
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
    #[flag(-f)]
    force: bool,
    /// Show what would happen without copying.
    #[flag]
    dry_run: bool,
    /// Number of parallel jobs.
    #[flag(--jobs, -j)]
    #[default(4)]
    jobs: usize,
    /// Retry count, or none to never retry.
    #[flag]
    retries: Option<u32>,
    /// Coloured output; `--no-color` turns it off.
    #[flag]
    #[default(true)]
    color: bool,
    /// Skip files matching a pattern.
    #[flag(-x)]
    exclude: Vec<String>,
    /// Follow symlinks (unset uses the default behaviour).
    #[flag]
    links: Option<bool>,
) {
    println!(
        "copy {from:?} -> {to:?} force={force} dry_run={dry_run} jobs={jobs} \
         retries={retries:?} color={color} exclude={exclude:?} links={links:?}"
    );
}
```

```text
$ copy a.txt
copy "a.txt" -> "." force=false dry_run=false jobs=4 retries=None color=true exclude=[] links=None
$ copy a.txt b.txt --jobs=8 -f -x '*.tmp'
copy "a.txt" -> "b.txt" force=true dry_run=false jobs=8 retries=None color=true exclude=["*.tmp"] links=None
```

Flags and positionals may be mixed in any order. A value flag takes `--flag value` or
`--flag=value`; a short flag takes `-j value` (not `-j8`). A repeated flag: the last one wins,
unless it is a `Vec`, which collects every occurrence. `--` ends flag parsing; everything after it
is a positional.

## Value types

Every parameter is parsed by `FromArg`. Values are parsed with `FromArg`, which is provided for
strings, paths, integers, floats, `bool`, `char`, `Duration` and the `std::net` addresses;
`#[derive(FromArg)]` handles fieldless enums; `#[parse(from_str)]` or `#[parse(fn)]` handle foreign
types; and your own types can implement `FromArg` directly.

```rust
use defcmd::{ArgError, FromArg, command};
use std::ffi::OsStr;
use std::time::Duration;

#[derive(Debug, FromArg)]
enum Mode {
    Fast,
    Careful,
}

#[derive(Debug)]
struct Percent(u8);

impl FromArg for Percent {
    fn from_arg(arg: &OsStr) -> Result<Self, ArgError> {
        let n: u8 = arg
            .to_str()
            .ok_or_else(|| ArgError::new("not valid UTF-8"))?
            .strip_suffix('%')
            .unwrap_or(arg.to_str().unwrap())
            .parse()
            .map_err(ArgError::new)?;
        if !(0..=100).contains(&n) {
            return Err(ArgError::new("must be between 0 and 100"));
        }
        Ok(Percent(n))
    }
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
    /// How much of the response to cache.
    #[flag]
    cache: Option<Percent>,
) {
    println!("fetch {url} timeout={timeout:?} mode={mode:?} cache={cache:?}");
}
```

```text
$ fetch https://example.com --timeout 1.5min --mode careful --cache 50%
fetch https://example.com timeout=900000000ns mode=Careful cache=Some(Percent(50))
$ fetch https://example.com --mode slow
Error: invalid value 'slow' for '--mode': expected one of: fast, careful
```

Help shows a parameter's type as `<type>` — `<duration>`, `<fast|careful>` for the enum,
`<percent>` for `Percent` — and its default from `Display`.

## Subcommands

`fn main` is the root command and Rust's entry point. Every other `#[command]` function is a
subcommand named after it; a path in `#[command("...")]` nests it deeper. Each function registers
itself, so there is no list of subcommands and no `parent` field to manage.

```rust
use defcmd::command;

/// A tiny todo list.
#[command]
fn main(
    /// Print extra detail.
    #[flag(global, -v)]
    verbose: bool,
) {
    println!("todo (verbose={verbose}); try `todo --help`");
}

/// Add an item.
#[command]
fn add(
    /// What needs doing.
    title: String,
    /// 1 (low) to 3 (high).
    #[flag(-p)]
    #[default(1)]
    priority: u8,
) {
    println!("add {title:?} priority={priority}");
}

/// List items.
#[command]
fn list(
    /// Include completed items.
    #[flag(-a)]
    all: bool,
    /// Print extra detail.
    #[flag(global, -v)]
    verbose: bool,
) {
    println!("list all={all} verbose={verbose}");
}

/// Add a remote list.
#[command("remote add")]
fn remote_add(
    /// Name of the remote.
    name: String,
    /// Where it lives.
    url: String,
) {
    println!("remote add {name} {url}");
}

/// Remove a remote list.
#[command("remote remove")]
fn remote_remove(
    /// Name of the remote.
    name: String,
) {
    println!("remote remove {name}");
}
```

```text
$ todo
todo (verbose=false); try `todo --help`
$ todo add "buy milk" -p 2
add "buy milk" priority=2
$ todo remote add origin https://example.com/repo
remote add origin "https://example.com/repo"
```

`todo --help` lists every descendant, flattened and sorted:

```text
A tiny todo list.

Usage:
  > todo {flags}

Subcommands:
  todo add - Add an item.
  todo list - List items.
  todo remote add - Add a remote list.
  todo remote remove - Remove a remote list.

Flags:
  -h, --help: Display the help message for this command
  -V, --version: Print the version
  -v, --verbose: Print extra detail.
```

A `#[command(alias = rm)]` adds another name for the same subcommand. The subcommand word is the
first positional after the command's own flags; if it matches both a subcommand and a positional,
the subcommand wins — use `--` to pass it as a value.

## Global flags

A flag of the root marked `#[flag(global)]` is accepted by every command, before or after the
subcommand word. A command that wants its value repeats the flag, with the same names and type;
`defcmd::check()` reports a repeat that does not match the root's. A command that does not repeat
it still accepts the flag, and its help lists it.

```rust
use defcmd::command;

/// Deploy services.
#[command]
fn main(
    /// Environment to target.
    #[flag(global, -e)]
    #[default("staging")]
    env: String,
) {
    println!("deploy env={env}");
}

/// Roll a service out.
#[command]
fn push(
    /// The service to roll out.
    service: String,
    /// Environment to target.
    #[flag(global, -e)]
    #[default("staging")]
    env: String,
) {
    println!("push {service} to {env}");
}
```

```text
$ deploy --env prod push api
push api to prod
$ deploy push api --env prod
push api to prod
```

## Return values and exit codes

A command returns `()`, [`std::process::ExitCode`], or a `Result` of either with an error that is
`Display`. `Err(e)` prints `Error: {e:#}` to stderr and exits 1; the `{:#}` form shows the whole
error chain of `anyhow::Error` while leaving plain `Display` types unaffected, so `String`,
`Box<dyn std::error::Error>` and friends all work without glue. Usage errors print an `Error:`
line and a `help:` line to stderr and exit 2.

A command that wants its own codes returns `ExitCode` or `Result<ExitCode, E>`. Nothing in a
command body is expected to call `std::process::exit`.

| Command returns | Result |
|---|---|
| `()` | exit 0 |
| `ExitCode` | exit with that code, nothing printed |
| `Result<T, E>` | `T` is any of these, `E: Display`; `Err(e)` prints `Error: {e:#}` and exits 1 |

Exit codes used by defcmd itself: `0` for success and `--help`/`--version` (written to stdout),
`1` when the command returned `Err`, and `2` for a usage error (written to stderr).

## Testing

```rust
use defcmd::{command, try_run_with};
use std::path::PathBuf;
use std::process::ExitCode;

#[command]
fn main() {}

#[command]
fn tool(file: PathBuf) -> Result<(), String> {
    let _ = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
    Ok(())
}

#[test]
fn tree_is_valid() {
    defcmd::check();
}

#[test]
fn a_missing_argument_is_reported() {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = try_run_with(["tool", "missing.conf"], &mut out, &mut err);
    assert_eq!(code, ExitCode::from(2));
    // A missing required argument is a usage error, written to stderr.
    let message = String::from_utf8(err).unwrap();
    assert!(message.starts_with("Error: missing required argument 'file'"));
}
```

`try_run` and `try_run_with` run the registered commands from an explicit argument list and return
the exit code; `try_run_with` also writes defcmd's own output (help, version and error messages)
to your buffers. `check()` validates the whole command tree once, in a test. Command discovery is a
link-time registry, so testing runs under `cargo test`; every command other than the empty root
`main` stays an ordinary function, so it can also be called directly.

## Examples

The [`defcmd/examples`](defcmd/examples) directory holds runnable documentation for each feature.
Run one and read its header, which shows the full help and error output:

```text
cargo run --example hello
cargo run --example copy
cargo run --example fetch
cargo run --example todo
cargo run --example deploy
```

Each example is named after the binary whose signature it shows; `copy` (a single file copy)
represents the `flags_and_arguments.rs` example, which also demonstrates the subcommands it contains.
```

## More

See the crate documentation ([`defcmd::command`](https://docs.rs/defcmd)) for the full reference to
parameter forms, return types, exit codes and the parsing rules.

## License

MIT. See [LICENSE](LICENSE).
