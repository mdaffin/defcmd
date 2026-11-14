//! Simplest possible CLI: a root command with one positional and one switch.
//!
//! ```text
//! $ hello World
//! Hello, World!
//! $ hello World --shout
//! HELLO, WORLD!
//! $ hello --help
//! $ hello --version
//! ```
//!
//! `fn main` is the root command, so there is no subcommand name to type.
//! The doc comment on `main` is the command's help text.

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
