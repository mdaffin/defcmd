# defcmd

A command is a plain Rust function marked `#[command]`, and its signature is the command line.
Modelled on nushell's `def main [...]`: parameters, flags, subcommands, help and error output
follow nushell's conventions. Targets macOS and Linux.

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

Subcommands come from function names (`#[command] fn add()`, or `#[command("remote add")]`),
and the tree is built from all commands at start-up. See the crate documentation for the
parameter forms, return types and exit codes, and `defcmd/examples/` for runnable examples:

| Example | Shows |
|---|---|
| `hello.rs` | the smallest possible command |
| `flags_and_arguments.rs` | parameters, flags, parsing rules, alias |
| `values.rs` | value types, enums, `FromStr` types, custom parsers |
| `nushell_style.rs` | a subcommand tree from names |
| `errors.rs` | return types, errors, exit codes |

## Not for v1

- Shell completions.
- Env var fallback for flags.
- Conflicts/requires constraints between parameters.
- Async commands.
- Passthrough of trailing arguments to external programs.
- Coloured help output.
- A root command other than `main` (one root per binary for now).
- A `FileSize` type for nushell's `filesize` values (`10mb`, `1kib`).
- Counted flags (`#[count]` is reserved).
- Single-file cargo scripts (`cargo -Zscript`, `rust-script`) are intended but not yet checked.

## License

MIT.
