//! Subcommands the way nushell scripts do it: the command's *name* is its place in the tree.
//!
//! ```text
//! # nushell                                  # defcmd
//! def main [] {}                             #[command] fn main() {}
//! def "main add" [] {}                       #[command] fn add() {}
//! def "main nest sub commands" [] {}         #[command("nest sub commands")] fn .. {}
//! ```
//!
//! nushell needs the `main` prefix because a bare `def add` is an ordinary internal command.
//! Here `#[command]` already marks a function as part of the CLI, so the prefix is implied.
//!
//! ```text
//! $ todo                              # runs `main` (no subcommand given)
//! $ todo add "buy milk" -p 2
//! $ todo list --all
//! $ todo remote add origin https://example.com/repo
//! $ todo remote remove origin
//! $ todo remote                       # a group: prints its help, exit 2
//! $ todo --verbose list               # `--verbose` belongs to `todo`; `list` never sees it
//! $ todo list --verbose               # exit 2: unknown flag for `list`
//! $ todo nosuch                       # exit 2: unexpected argument (`todo` takes no positional)
//! $ todo -- list                      # exit 2 too: after `--`, `list` is a positional
//! ```
//!
//! Rules:
//! - The function named `main` is the root. It is the same `main` as Rust's entry point.
//!   A subcommand cannot be called `main` (compile error).
//! - Every other `#[command]` is a subcommand of the root. A bare `#[command]` uses the
//!   function name, with `_` shown as `-`. Commands nested deeper give their path as a
//!   string, relative to the root: `#[command("remote add")]`.
//! - The Rust function name is never shown for string-named commands, so those can be named
//!   anything that does not clash.
//! - There is no list of subcommands and no `parent = ..`: each function registers itself
//!   (link-time registry, via `inventory`) and the tree is built from the name prefixes.
//! - A missing middle node (here `remote`) is created implicitly as a help-only group. This
//!   differs from nushell, which has no groups; it keeps `todo remote --help` useful.
//! - The subcommand word is the first positional after the command's own flags. If it matches
//!   a subcommand name, the subcommand wins (as in nushell). Use `--` to pass such a word as
//!   a positional. If it matches nothing and the command takes no positional, that is a usage
//!   error.
//! - Flags are not inherited: `todo --verbose list` sets the root's flag, not `list`'s. The
//!   root's body does not run when a subcommand does, so that flag is only validated.
//! - The whole tree is validated at every start-up (duplicate names and aliases, more than
//!   one root), so mistakes surface on the first run rather than in some rare path.
//!
//! `todo --help` lists every descendant by its full path, flattened and sorted:
//!
//! ```text
//! A tiny todo list.
//!
//! Usage:
//!   > todo {flags}
//!
//! Subcommands:
//!   todo add - Add an item.
//!   todo list - List items.
//!   todo remote add - Add a remote list.
//!   todo remote remove - Remove a remote list.
//!
//! Flags:
//!   -h, --help: Display the help message for this command
//!   -V, --version: Print the version
//!   -v, --verbose: Print extra detail.
//! ```
//!
//! `todo add --help`:
//!
//! ```text
//! Add an item.
//!
//! Usage:
//!   > todo add {flags} <title>
//!
//! Flags:
//!   -h, --help: Display the help message for this command
//!   -V, --version: Print the version
//!   -p, --priority <int>: 1 (low) to 3 (high). (default: 1)
//!
//! Parameters:
//!   title <string>: What needs doing.
//! ```
//!
//! The implicit group `remote` has no description, so `todo remote --help` (or `todo remote`,
//! which also exits 2) starts at `Usage:`:
//!
//! ```text
//! Usage:
//!   > todo remote
//!
//! Subcommands:
//!   todo remote add - Add a remote list.
//!   todo remote remove - Remove a remote list.
//!
//! Flags:
//!   -h, --help: Display the help message for this command
//!   -V, --version: Print the version
//! ```

use defcmd::command;

/// A tiny todo list.
#[command]
fn main(
    /// Print extra detail.
    #[flag(-v)]
    verbose: bool,
) {
    // Runs only when no subcommand is given.
    println!("todo (verbose={verbose}); try `todo --help`");
}

/// Add an item.
#[command] // `add`
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
#[command] // `list`
fn list(
    /// Include completed items.
    #[flag(-a)]
    all: bool,
) {
    println!("list all={all}");
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
