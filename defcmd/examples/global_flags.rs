//! Global flags: flags of the root that every command accepts.
//!
//! ```text
//! $ deploy push api                         # staging, for real
//! $ deploy push api --env prod              # a global flag after the subcommand word
//! $ deploy --env prod push api              # the same, before it
//! $ deploy --dry-run push api -e prod       # both places at once
//! $ deploy -v status                        # `status` does not read --verbose, but accepts it
//! $ deploy status --env prod                # likewise
//! $ deploy --dry-run                        # the root's own body sees them too
//! $ deploy --local push api                 # exit 2: `--local` is not global, so it cannot
//!                                           # come before a subcommand word
//! ```
//!
//! Rules:
//! - `#[flag(global)]` on a parameter of `main` makes the flag global. It takes the same names
//!   as `#[flag]`: `#[flag(global, -e)]`, `#[flag(global, -e, --env)]`.
//! - A command that wants the value repeats the flag, with the same names and type. Rust
//!   functions cannot see the root's parameters, so this is how the value reaches `push`.
//!   `defcmd::check()` reports a repeat that does not match the root's flag.
//! - A command that does not repeat the flag still accepts it (here `status` and `-v`) and
//!   ignores it. Its `--help` lists the flag all the same.
//! - The value may be given before or after the subcommand word, or both. A repeatable flag
//!   (`Vec<T>`) collects every occurrence; for any other flag the last one wins.
//! - Each command uses its own `#[default]`. Keep them equal, as `env` does here.
//! - A root flag that is not global, like `--local`, belongs to the root's body alone. That
//!   body does not run when a subcommand does, so the flag cannot come before a subcommand
//!   word: that is a usage error rather than a flag that silently does nothing.
//!
//! `deploy status --help` shows the flags `status` accepts without declaring them:
//!
//! ```text
//! Show what is deployed.
//!
//! Usage:
//!   > deploy status {flags}
//!
//! Flags:
//!   -h, --help: Display the help message for this command
//!   -V, --version: Print the version
//!   -v, --verbose: Print extra detail.
//!   --dry-run: Show what would happen without doing it.
//!   -e, --env <string>: Environment to target. (default: 'staging')
//! ```

use defcmd::command;

/// Deploy services.
#[command]
fn main(
    /// Print extra detail.
    #[flag(global, -v)]
    verbose: bool,
    /// Show what would happen without doing it.
    #[flag(global)]
    dry_run: bool,
    /// Environment to target.
    #[flag(global, -e)]
    #[default("staging")]
    env: String,
    /// Only for the root: not global.
    #[flag]
    local: bool,
) {
    // Runs only when no subcommand is given.
    println!("deploy verbose={verbose} dry_run={dry_run} env={env} local={local}");
}

/// Roll a service out.
#[command]
fn push(
    /// The service to roll out.
    service: String,
    /// Show what would happen without doing it.
    #[flag(global)]
    dry_run: bool,
    /// Environment to target.
    #[flag(global, -e)]
    #[default("staging")]
    env: String,
) {
    let verb = if dry_run { "would push" } else { "push" };
    println!("{verb} {service} to {env}");
}

/// Show what is deployed.
#[command]
fn status() {
    println!("status");
}
